use crate::{browser_access::BrowserError, web_state::WebState};
use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use std::time::Instant;
pub async fn run(
    app: WebState,
    action: impl FnOnce(&WebState) -> Result<String, BrowserError> + Send + 'static,
) -> Result<String, Response> {
    // One expensive KDF at a time; retain the permit inside the blocking worker
    // even if HTTP cancellation/timeout drops its awaiting future.
    let permit = app
        .password_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| throttled(1))?;
    let remaining = app
        .login_throttle
        .lock()
        .map_err(|_| unavailable())?
        .remaining(Instant::now());
    if remaining > 0 {
        return Err(throttled(remaining));
    }
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let result = action(&app);
        let mut throttle = app
            .login_throttle
            .lock()
            .map_err(|_| BrowserError::Unavailable)?;
        match &result {
            Ok(_) | Err(BrowserError::Input(_)) => throttle.success(),
            Err(BrowserError::Invalid) => throttle.failure(Instant::now()),
            Err(BrowserError::Unavailable) => {}
        }
        result
    })
    .await
    .map_err(|_| unavailable())?;
    result.map_err(|e| match e {
        BrowserError::Invalid => (
            StatusCode::UNAUTHORIZED,
            "Password not accepted. Try again.",
        )
            .into_response(),
        BrowserError::Input(message) => (StatusCode::UNPROCESSABLE_ENTITY, message).into_response(),
        BrowserError::Unavailable => unavailable(),
    })
}
fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Sign-in is temporarily unavailable. Try again.",
    )
        .into_response()
}
fn throttled(seconds: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, seconds.to_string())],
        format!("Please wait {seconds} seconds before trying again."),
    )
        .into_response()
}
