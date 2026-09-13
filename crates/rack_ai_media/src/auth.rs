use crate::{
    config::Principal,
    types::digest,
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
    let hash = crate::browser_sessions::cookie_digest(headers)?;
    crate::browser_sessions::principal(app, &hash).map(|p| (p, true))
}

pub(crate) fn token_principal(app: &WebState, token: &str) -> Result<Principal, String> {
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
    // HTTP/2 module preloads arrive in bursts. Bound their waiting room while
    // keeping API/status admission independent of native assets.
    let native_host = app
        .config
        .native_origin
        .split_once("://")
        .map(|(_, h)| h.trim_end_matches('/'));
    let _native_waiter;
    let _permit;
    if native_host == Some(host) {
        _native_waiter = match app.native_waiters.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        };
        _permit = match tokio::time::timeout(
            std::time::Duration::from_secs(crate::limits::NATIVE_WAIT_SECONDS),
            app.native_connections.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(p)) => p,
            _ => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        };
    } else {
        _permit = match app.connections.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        };
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
