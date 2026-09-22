use crate::{
    config::Principal,
    types::{Mode, ServiceState, now},
    web_state::{NativeStaticGrant, WebState, blocking},
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
    let websocket = request
        .headers()
        .get(header::UPGRADE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.eq_ignore_ascii_case("websocket"));
    let frontend_read = !websocket && frontend_read_request(request.method(), request.uri().path());
    let read = app.clone();
    let result = blocking(move || authorize_native(&read, &principal, frontend_read)).await;
    let secret = match result {
        Ok(Access::Ready(secret)) => secret,
        Ok(Access::Temporary(message)) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                [(header::RETRY_AFTER, "2")],
                format!("ComfyUI {message}. Your Rack AI session is retained."),
            )
                .into_response();
        }
        _ => {
            return (
                StatusCode::CONFLICT,
                axum::response::Html(
                    "<h1>ComfyUI session is not ready</h1><p>Use the Rack AI launcher to inspect your session.</p>",
                ),
            )
                .into_response();
        }
    };
    let input = AuthorizedNative { request, secret };
    if websocket {
        crate::native_upgrade::handle(app, input).await
    } else {
        crate::native_http::handle(app, input).await
    }
}

fn authorize_native(
    app: &WebState,
    principal: &Principal,
    frontend_read: bool,
) -> Result<Access, String> {
    let state = app.store.read()?;
    if state.service.mode != Mode::Interactive {
        clear_static_grant(app);
        return Ok(Access::Absent);
    }
    let Some(session_id) = state.service.session.clone() else {
        clear_static_grant(app);
        return Ok(Access::Absent);
    };
    if !state.sessions.iter().any(|s| {
        s.id == session_id && s.owner == principal.id && !s.stopped && !s.release_requested
    }) {
        clear_static_grant(app);
        return Ok(Access::Absent);
    }
    let message = match state.service.state {
        ServiceState::Restarting => Some("Restarting"),
        ServiceState::Starting => Some("Starting"),
        ServiceState::Draining | ServiceState::Stopping => Some("Finishing"),
        ServiceState::Ready => None,
        _ => {
            clear_static_grant(app);
            return Ok(Access::Absent);
        }
    };
    if let Some(message) = message {
        return Ok(Access::Temporary(message));
    }
    let Some(invocation) = state.service.invocation.clone() else {
        clear_static_grant(app);
        return Ok(Access::Absent);
    };
    if frontend_read {
        if let Some(secret) = cached_static_grant(
            app,
            &principal.id,
            &session_id,
            &state.service.activation,
            &invocation,
        ) {
            return Ok(Access::Ready(secret));
        }
    }
    let runtime = crate::runtime::Runtime::new((*app.config).clone())?;
    crate::lifecycle::Lifecycle { runtime: &runtime }.verify(&state.service)?;
    if let Some(handle) = &state.service.lease {
        rack_ai_infrastructure::managed_lease::ManagedLease {
            resources: &runtime.reservations,
        }
        .verify(handle, true)?;
    }
    let secret = std::fs::read_to_string(&app.config.control_secret_file)
        .map(|s| s.trim().to_string())
        .map_err(|e| e.to_string())?;
    if frontend_read {
        remember_static_grant(
            app,
            NativeStaticGrant {
                principal_id: principal.id.clone(),
                session_id,
                activation: state.service.activation.clone(),
                invocation,
                secret: secret.clone(),
                expires_at: now() + crate::limits::NATIVE_STATIC_GRANT_SECONDS,
            },
        );
    }
    Ok(Access::Ready(secret))
}

pub(crate) fn frontend_read_request(method: &Method, path: &str) -> bool {
    matches!(*method, Method::GET | Method::HEAD) && path != "/ws"
}

fn cached_static_grant(
    app: &WebState,
    principal_id: &str,
    session_id: &str,
    activation: &str,
    invocation: &str,
) -> Option<String> {
    let cache = app.native_static_grant.lock().ok()?;
    let grant = cache.as_ref()?;
    (grant.expires_at > now()
        && grant.principal_id == principal_id
        && grant.session_id == session_id
        && grant.activation == activation
        && grant.invocation == invocation)
        .then(|| grant.secret.clone())
}

fn remember_static_grant(app: &WebState, grant: NativeStaticGrant) {
    if let Ok(mut cache) = app.native_static_grant.lock() {
        *cache = Some(grant);
    }
}

fn clear_static_grant(app: &WebState) {
    if let Ok(mut cache) = app.native_static_grant.lock() {
        *cache = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_read_classifier_covers_bootstrap_gets_only() {
        assert!(frontend_read_request(&Method::GET, "/"));
        assert!(frontend_read_request(&Method::HEAD, "/assets/index.js"));
        assert!(frontend_read_request(&Method::GET, "/fonts/material.woff2"));
        assert!(frontend_read_request(
            &Method::GET,
            "/api/userdata/user.css"
        ));
        assert!(frontend_read_request(&Method::GET, "/object_info"));
        assert!(frontend_read_request(&Method::GET, "/system_stats"));
        assert!(frontend_read_request(&Method::GET, "/queue"));
        assert!(frontend_read_request(&Method::GET, "/history/abc"));
        assert!(frontend_read_request(&Method::GET, "/global_subgraphs"));
        assert!(!frontend_read_request(&Method::POST, "/assets/index.js"));
        assert!(!frontend_read_request(&Method::POST, "/prompt"));
        assert!(!frontend_read_request(&Method::GET, "/ws"));
    }
}
