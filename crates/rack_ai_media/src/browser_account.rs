use crate::{
    auth,
    browser_access::{self, ChangeRequest, PasswordChange},
    browser_attempt, browser_pages, browser_sessions,
    web_state::{WebState, blocking},
};
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
pub async fn account(State(app): State<WebState>, request: Request) -> Response {
    if request.headers().contains_key(header::AUTHORIZATION) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request.method() == Method::GET {
        let store = app.human.clone();
        return match blocking(move || store.read()).await {
            Ok(record) => browser_pages::page(
                include_str!("../web/account.html"),
                record.password_hash.is_none(),
            ),
            Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        };
    }
    if request.method() != Method::POST || !auth::origin_allowed(request.headers(), &app) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let session_digest = match browser_sessions::cookie_digest(request.headers()) {
        Ok(value) => value,
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let bytes =
        match axum::body::to_bytes(request.into_body(), crate::limits::ACCOUNT_BODY_BYTES).await {
            Ok(v) => v,
            Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        };
    let form: PasswordChange = match serde_urlencoded::from_bytes(&bytes) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match browser_attempt::run(app.clone(), move |app| {
        browser_access::change(
            app,
            ChangeRequest {
                form,
                session_digest,
            },
        )
    })
    .await
    {
        Ok(token) => browser_pages::signed_in(&app, (&token, "/account?saved=1")),
        Err(response) => response,
    }
}
pub async fn logout(State(app): State<WebState>, request: Request) -> Response {
    if request.method() != Method::POST
        || request.headers().contains_key(header::AUTHORIZATION)
        || !auth::origin_allowed(request.headers(), &app)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let digest = match browser_sessions::cookie_digest(request.headers()) {
        Ok(value) => value,
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let store = app.store.clone();
    if blocking(move || {
        store.update(|s| {
            s.browsers.retain(|b| b.digest != digest);
            Ok(())
        })
    })
    .await
    .is_err()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    (
        StatusCode::SEE_OTHER,
        [
            (
                header::SET_COOKIE,
                browser_sessions::expired_cookie(app.config.public_origin.starts_with("https:")),
            ),
            (header::LOCATION, "/login".into()),
        ],
    )
        .into_response()
}
