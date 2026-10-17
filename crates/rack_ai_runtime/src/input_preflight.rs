//! Opt-in, non-generating input accounting for an owned frozen LLM profile.
mod access;
mod backend;
mod report;

use crate::{
    protocol::{Payload, Protocol},
    service::Service,
    types::Demand,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
pub(crate) use report::Report;
use serde_json::{Value, json};
use std::sync::Arc;

pub async fn handle(
    State(service): State<Arc<Service>>,
    Path((reservation, capability)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let source = match crate::api::authenticate(&service.config, &headers) {
        Ok(source) => source,
        Err(_) => return failure("unauthorized".into()),
    };
    let Ok(permit) = service.gateway_waiters.clone().try_acquire_owned() else {
        return failure("capacity_gateway_waiters".into());
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let demand = access::read(
            &service,
            &access::Access {
                owner: source.source,
                reservation,
                capability,
            },
        )?;
        let payload = payload(&demand, body)?;
        check(&service, (&demand, &payload))
    })
    .await;
    match result {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(error)) => failure(error),
        Err(_) => failure("input_preflight_worker_failure".into()),
    }
}

pub(crate) fn payload(demand: &Demand, mut body: Value) -> Result<Payload, String> {
    let object = body.as_object_mut().ok_or("invalid_protocol_body")?;
    if !["max_tokens", "max_completion_tokens", "max_output_tokens"]
        .iter()
        .any(|key| object.contains_key(*key))
    {
        object.insert("max_tokens".into(), json!(demand.profile.max_output_tokens));
    }
    let payload = Payload {
        protocol: Protocol::ChatCompletions,
        body,
    };
    if payload.validate(demand)? == 0 {
        return Err("inference_limits".into());
    }
    access::text_only(&payload)?;
    Ok(payload)
}

pub(crate) fn check(service: &Service, input: (&Demand, &Payload)) -> Result<Report, String> {
    let (demand, payload) = input;
    access::verify(service, demand)?;
    let requested = payload.validate(demand)?;
    if requested == 0 {
        return Err("inference_limits".into());
    }
    access::text_only(payload)?;
    let _guard = rack_ai_infrastructure::endpoint_fence::managed_read_guard(
        &service.config.authority_root,
        &demand.profile.endpoint,
    )?;
    access::verify(service, demand)?;
    let process = demand
        .process
        .as_ref()
        .ok_or("input_preflight_backend_identity_unproven")?;
    crate::process::endpoint_owned(process, &demand.profile)
        .map_err(|_| "input_preflight_backend_identity_unproven")?;
    let tokens = backend::count(service, (demand, payload))?;
    crate::process::endpoint_owned(process, &demand.profile)
        .map_err(|_| "input_preflight_backend_identity_unproven")?;
    access::verify(service, demand)?;
    Ok(Report::new(demand, (requested, tokens)))
}

fn failure(error: String) -> axum::response::Response {
    let status = match error.as_str() {
        "unauthorized" => StatusCode::UNAUTHORIZED,
        "not_found" => StatusCode::NOT_FOUND,
        "invalid_scoped_capability" => StatusCode::FORBIDDEN,
        "capacity_gateway_waiters" => StatusCode::TOO_MANY_REQUESTS,
        "reservation_not_dispatchable"
        | "stale_generation_or_profile"
        | "input_preflight_unqualified"
        | "input_preflight_backend_identity_unproven"
        | "input_preflight_endpoint_busy" => StatusCode::CONFLICT,
        error
            if error.starts_with("input_preflight_backend")
                || error.starts_with("input_preflight_invalid_")
                || error == "input_preflight_response_oversized"
                || error == "input_preflight_worker_failure"
                || error == "input_preflight_endpoint_unavailable" =>
        {
            StatusCode::BAD_GATEWAY
        }
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(json!({"schema":"rack-ai/input-preflight/v1", "error":error})),
    )
        .into_response()
}
