use crate::{
    config::Principal,
    types::{BrowserSession, digest, identity, now},
    web_state::{WebState, blocking},
};
use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use subtle::ConstantTimeEq;
pub fn authorized(
    headers: &axum::http::HeaderMap,
    app: &WebState,
) -> Result<(Principal, bool), String> {
    // Proxy identity headers never authenticate a caller; only our credentials do.
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let token = value
            .to_str()
            .map_err(|_| "unauthorized")?
            .strip_prefix("Bearer ")
            .ok_or("unauthorized")?;
        return token_principal(app, token).map(|p| (p, false));
    }
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let value = cookie
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == "rack_session")
        .map(|(_, v)| v)
        .ok_or("unauthorized")?;
    let hash = digest(value.as_bytes());
    let state = app.store.read()?;
    let session = state
        .browsers
        .iter()
        .find(|s| s.expires > now() && bool::from(s.digest.as_bytes().ct_eq(hash.as_bytes())))
        .ok_or("unauthorized")?;
    app.config
        .principals
        .iter()
        .find(|p| p.id == session.owner && p.operator)
        .cloned()
        .map(|p| (p, true))
        .ok_or("unauthorized".into())
}
fn token_principal(app: &WebState, token: &str) -> Result<Principal, String> {
    let hash = digest(token.as_bytes());
    app.config
        .principals
        .iter()
        .find(|p| bool::from(p.token_sha256.as_bytes().ct_eq(hash.as_bytes())))
        .cloned()
        .ok_or("unauthorized: invalid credential".into())
}
pub fn origin_allowed(headers: &axum::http::HeaderMap, app: &WebState) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == app.config.public_origin || v == app.config.native_origin)
}
pub async fn guard(State(app): State<WebState>, mut request: Request, next: Next) -> Response {
    let Ok(_permit) = app.connections.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let allowed = [&app.config.public_origin, &app.config.native_origin]
        .iter()
        .any(|o| {
            o.split_once("://")
                .is_some_and(|(_, authority)| authority.trim_end_matches('/') == host)
        });
    if !allowed {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if request.uri().path() == "/login" {
        return next.run(request).await;
    }
    let headers = request.headers().clone();
    let auth_app = app.clone();
    let result = blocking(move || authorized(&headers, &auth_app)).await;
    let (principal, browser) = match result {
        Ok(value) => value,
        Err(_) => {
            return if request.uri().path().starts_with("/api/media/") {
                (StatusCode::UNAUTHORIZED,Json(serde_json::json!({"error":{"code":"unauthorized","message":"Authentication required"}}))).into_response()
            } else {
                Redirect::to("/login").into_response()
            };
        }
    };
    if browser
        && request.method() != axum::http::Method::GET
        && request.method() != axum::http::Method::HEAD
        && !origin_allowed(request.headers(), &app)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    request.extensions_mut().insert(principal);
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        header::HeaderValue::from_static("nosniff"),
    );
    response
}
#[derive(serde::Deserialize)]
struct Login {
    credential: String,
}
pub async fn login(State(app): State<WebState>, request: Request) -> Response {
    if request.method() == axum::http::Method::GET {
        return axum::response::Html(include_str!("../web/login.html")).into_response();
    }
    if request.method() != axum::http::Method::POST || !origin_allowed(request.headers(), &app) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let bytes =
        match axum::body::to_bytes(request.into_body(), crate::limits::LOGIN_BODY_BYTES).await {
            Ok(v) => v,
            Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        };
    let input: Login = match serde_urlencoded::from_bytes(&bytes) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let result = blocking(move || {
        let principal = token_principal(&app, &input.credential)?;
        if !principal.operator {
            return Err("operator credential required".into());
        }
        let token = identity() + &identity();
        let secure = app.config.public_origin.starts_with("https:");
        app.store.update(|s| {
            s.browsers.retain(|b| b.expires > now());
            if s.browsers.len() >= crate::limits::BROWSER_SESSIONS {
                return Err("browser session limit reached".into());
            }
            s.browsers.push(BrowserSession {
                digest: digest(token.as_bytes()),
                owner: principal.id,
                expires: now() + crate::limits::COOKIE_SECONDS,
            });
            Ok(())
        })?;
        Ok(format!(
            "rack_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=43200{}",
            if secure { "; Secure" } else { "" }
        ))
    })
    .await;
    match result {
        Ok(cookie) => (
            [(header::SET_COOKIE, cookie), (header::LOCATION, "/".into())],
            StatusCode::SEE_OTHER,
        )
            .into_response(),
        Err(_) => {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            StatusCode::UNAUTHORIZED.into_response()
        }
    }
}
