use crate::{browser_sessions, web_state::WebState};
use axum::{
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
pub fn page(template: &str, bootstrap: bool) -> Response {
    let note = if bootstrap {
        "Use the existing operator credential once, then set a password."
    } else {
        ""
    };
    let title = if bootstrap {
        "Set password"
    } else {
        "Change password"
    };
    Html(
        template
            .replace("{{bootstrap_note}}", note)
            .replace("{{password_action}}", title),
    )
    .into_response()
}
pub fn signed_in(app: &WebState, input: (&str, &str)) -> Response {
    (
        StatusCode::SEE_OTHER,
        [
            (
                header::SET_COOKIE,
                browser_sessions::cookie(input.0, app.config.public_origin.starts_with("https:")),
            ),
            (header::LOCATION, input.1.to_string()),
        ],
    )
        .into_response()
}
pub fn security_headers(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        header::HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        "referrer-policy",
        header::HeaderValue::from_static("no-referrer"),
    );
}
