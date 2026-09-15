use crate::{
    config::Principal,
    types::{Mode, ServiceState},
    web_state::{WebState, blocking},
};
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
pub struct AuthorizedNative {
    pub request: Request,
    pub secret: String,
}
enum Access {
    Ready(String),
    Temporary(&'static str),
    Absent,
}
pub async fn handle(State(app): State<WebState>, request: Request) -> Response {
    let Some(principal) = request.extensions().get::<Principal>().cloned() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if crate::restart_request::is_restart_path(request.uri().path()) {
        if request.method() != Method::POST {
            return StatusCode::METHOD_NOT_ALLOWED.into_response();
        }
        let restart_app = app.clone();
        return match blocking(move || {
            let runtime = crate::runtime::Runtime::new((*restart_app.config).clone())?;
            crate::restart_request::RestartRequest { runtime: &runtime }.begin(&principal)
        })
        .await
        {
            Ok(id) => (
                StatusCode::ACCEPTED,
                axum::Json(serde_json::json!({"status":"restarting","restart_id":id})),
            )
                .into_response(),
            Err(message) => (
                StatusCode::CONFLICT,
                axum::Json(serde_json::json!({"error":message})),
            )
                .into_response(),
        };
    }
    let read = app.clone();
    let result = blocking(move || {
        let state = read.store.read()?;
        if state.service.mode != Mode::Interactive
            || !state.sessions.iter().any(|s| {
                Some(&s.id) == state.service.session.as_ref()
                    && s.owner == principal.id
                    && !s.stopped
                    && !s.release_requested
            })
        {
            return Ok(Access::Absent);
        }
        let message = match state.service.state {
            ServiceState::Restarting => Some("Restarting"),
            ServiceState::Starting => Some("Starting"),
            ServiceState::Draining | ServiceState::Stopping => Some("Finishing"),
            ServiceState::Ready => None,
            _ => return Ok(Access::Absent),
        };
        if let Some(message) = message {
            return Ok(Access::Temporary(message));
        }
        let runtime = crate::runtime::Runtime::new((*read.config).clone())?;
        crate::lifecycle::Lifecycle { runtime: &runtime }.verify(&state.service)?;
        if let Some(handle) = &state.service.lease {
            rack_ai_infrastructure::managed_lease::ManagedLease {
                resources: &runtime.reservations,
            }
            .verify(handle, true)?;
        }
        std::fs::read_to_string(&read.config.control_secret_file)
            .map(|s| Access::Ready(s.trim().to_string()))
            .map_err(|e| e.to_string())
    })
    .await;
    let secret = match result {
        Ok(Access::Ready(secret)) => secret,
        Ok(Access::Temporary(message)) => return (StatusCode::SERVICE_UNAVAILABLE,
            [(header::RETRY_AFTER, "2")], format!("ComfyUI {message}. Your Rack AI session is retained.")).into_response(),
        _ => return (StatusCode::CONFLICT, axum::response::Html(
            "<h1>ComfyUI session is not ready</h1><p>Use the Rack AI launcher to inspect your session.</p>")).into_response(),
    };
    let websocket = request
        .headers()
        .get(header::UPGRADE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.eq_ignore_ascii_case("websocket"));
    let input = AuthorizedNative { request, secret };
    if websocket {
        crate::native_upgrade::handle(app, input).await
    } else {
        crate::native_http::handle(app, input).await
    }
}
