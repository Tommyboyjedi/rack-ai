use crate::{
    service::Service,
    types::{Invocation, InvocationState, VERSION},
};
use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use serde_json::json;

#[derive(Serialize)]
struct Detail {
    #[serde(skip_serializing_if = "Option::is_none")]
    service: Option<String>,
    phase: &'static str,
    category: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<InvocationState>,
    reason: String,
    recoverable: bool,
}

pub fn generic(error: String) -> axum::response::Response {
    response(
        error.clone(),
        Detail {
            service: None,
            phase: phase(&error),
            category: category(&error),
            state: None,
            recoverable: recoverable(&error),
            reason: error,
        },
    )
}

pub fn invocation(service: &Service, invocation: &Invocation) -> axum::response::Response {
    let reason = invocation
        .error
        .clone()
        .unwrap_or_else(|| format!("invocation_{:?}", invocation.state));
    invocation_response(service, (invocation, reason))
}

pub fn wait_expired(service: &Service, invocation: &Invocation) -> axum::response::Response {
    invocation_response(service, (invocation, "gateway_wait_expired".into()))
}

fn invocation_response(
    service: &Service,
    input: (&Invocation, String),
) -> axum::response::Response {
    let (invocation, reason) = input;
    let tag = service
        .inspect(&invocation.owner, &invocation.request.reservation_id)
        .ok()
        .map(|demand| demand.profile.tag);
    response(
        "invocation_failed".into(),
        Detail {
            service: tag,
            phase: phase(&reason),
            category: category(&reason),
            state: Some(invocation.state),
            recoverable: recoverable(&reason),
            reason,
        },
    )
}

fn response(error: String, detail: Detail) -> axum::response::Response {
    let status = if error.starts_with("capacity_") {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::CONFLICT
    };
    (
        status,
        Json(json!({"schema":VERSION,"error":error,"detail":detail})),
    )
        .into_response()
}

fn phase(reason: &str) -> &'static str {
    if reason.contains("admission") || reason.contains("limit") {
        "admission"
    } else if reason.contains("reasoning")
        || reason.contains("protocol")
        || reason.contains("model")
    {
        "normalization"
    } else if reason.starts_with("reservation_") {
        "reservation"
    } else {
        "inference"
    }
}

fn category(reason: &str) -> &'static str {
    if reason.contains("limit") {
        "request_limits"
    } else if reason.contains("reasoning")
        || reason.contains("protocol")
        || reason.contains("model")
    {
        "request_semantics"
    } else if reason.starts_with("backend_") {
        "backend_failure"
    } else if reason.starts_with("reservation_") {
        "reservation_state"
    } else {
        "invocation_failure"
    }
}

fn recoverable(reason: &str) -> bool {
    reason.contains("transport")
        || reason.contains("unavailable")
        || reason.contains("wait_expired")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categories_preserve_reason_without_treating_limits_as_recoverable() {
        assert_eq!(phase("input_admission_limits_exceeded"), "admission");
        assert_eq!(
            category("input_admission_limits_exceeded"),
            "request_limits"
        );
        assert!(!recoverable("input_admission_limits_exceeded"));
        assert!(recoverable("backend_transport_uncertain"));
    }
}
