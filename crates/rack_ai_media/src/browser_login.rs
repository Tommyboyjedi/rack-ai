use crate::{
    auth, browser_access, browser_attempt, browser_pages,
    web_state::{WebState, blocking},
};
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    #[serde(alias = "credential")]
    password: String,
}
pub async fn handle(State(app): State<WebState>, request: Request) -> Response {
    let mut response = dispatch(app, request).await;
    browser_pages::security_headers(&mut response);
    response
}
async fn dispatch(app: WebState, request: Request) -> Response {
    if request.method() == Method::GET {
        let store = app.human.clone();
        return match blocking(move || store.read()).await {
            Ok(record) => browser_pages::page(
                include_str!("../web/login.html"),
                record.password_hash.is_none(),
            ),
            Err(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Sign-in is temporarily unavailable.",
            )
                .into_response(),
        };
    }
    if request.method() != Method::POST || !auth::origin_allowed(request.headers(), &app) {
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
    match browser_attempt::run(app.clone(), move |app| {
        browser_access::login(app, &input.password)
    })
    .await
    {
        Ok(token) => browser_pages::signed_in(&app, (&token, "/")),
        Err(response) => response,
    }
}
