//! Capability URLs rotate with activation. Explicit keys identify logical calls within a reservation.
use crate::{
    protocol::{Payload, Protocol},
    service::Service,
    types::*,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde_json::{Value, json};
use std::sync::Arc;
pub async fn handle(
    State(service): State<Arc<Service>>,
    Path((id, generation, path)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let Ok(_permit) = service.gateway_waiters.clone().try_acquire_owned() else {
        return failure("capacity_gateway_waiters".into());
    };
    let (namespace, path) = if let Some(rest) = path.strip_prefix("calls/") {
        let Some((key, path)) = rest.split_once('/') else {
            return failure("invalid_call_namespace".into());
        };
        if !valid_id(key) {
            return failure("invalid_call_namespace".into());
        }
        (Some(key.to_string()), path)
    } else {
        (None, path.as_str())
    };
    let protocol = match path {
        "chat/completions" => Protocol::ChatCompletions,
        "responses" => Protocol::Responses,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let key = headers
        .get("idempotency-key")
        .and_then(|h| h.to_str().ok())
        .map(String::from);
    let copy = service.clone();
    let submitted = tokio::task::spawn_blocking(move || {
        submit(
            &copy,
            GatewayCall {
                id,
                generation,
                protocol,
                body,
                key,
                namespace,
            },
        )
    })
    .await;
    let invocation = match submitted {
        Ok(Ok(i)) => i,
        Ok(Err(e)) => return failure(e),
        Err(_) => return failure("receiver_failure".into()),
    };
    // A disconnected caller does not resubmit or cancel durable work. A retry reconciles the same identity.
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(
            service.config.limits.max_wait_seconds + invocation.request.timeout_seconds + 2,
        );
    loop {
        let copy = service.clone();
        let owner = invocation.owner.clone();
        let id = invocation.id.clone();
        let result = tokio::task::spawn_blocking(move || copy.result(&owner, &id)).await;
        let result = match result {
            Ok(Ok(i)) => i,
            Ok(Err(e)) => return failure(e),
            Err(_) => return failure("receiver_failure".into()),
        };
        if result.state == InvocationState::Completed {
            let Some(value) = result.result else {
                return failure("missing_protocol_result".into());
            };
            let response = &value["rack_protocol_response"];
            let content_type = response["content_type"]
                .as_str()
                .unwrap_or("application/json")
                .to_string();
            return (
                [("content-type", content_type)],
                response["body"].as_str().unwrap_or("").to_string(),
            )
                .into_response();
        }
        if matches!(
            result.state,
            InvocationState::Uncertain | InvocationState::Cancelled | InvocationState::Expired
        ) || std::time::Instant::now() >= deadline
        {
            return failure(format!("invocation_{:?}:{}", result.state, result.id));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
struct GatewayCall {
    id: String,
    generation: String,
    protocol: Protocol,
    body: Value,
    key: Option<String>,
    namespace: Option<String>,
}
fn submit(service: &Service, call: GatewayCall) -> Result<Invocation, String> {
    let d = service.authority.read(|s| {
        let d = s
            .data
            .demands
            .get(&call.id)
            .ok_or("invalid_scoped_capability")?;
        use subtle::ConstantTimeEq;
        if !bool::from(d.access_key.as_bytes().ct_eq(call.generation.as_bytes()))
            || !crate::service::active(d)
        {
            return Err("invalid_scoped_capability".into());
        }
        Ok(d.clone())
    })?;
    let mut body = call.body;
    let object = body.as_object_mut().ok_or("invalid_protocol_body")?;
    if !["max_tokens", "max_completion_tokens", "max_output_tokens"]
        .iter()
        .any(|k| object.contains_key(*k))
    {
        let key = match call.protocol {
            Protocol::ChatCompletions => "max_tokens",
            Protocol::Responses => "max_output_tokens",
        };
        object.insert(key.into(), json!(d.profile.max_output_tokens));
    }
    let payload = Payload {
        protocol: call.protocol,
        body,
    };
    let max_tokens = payload.validate(&d)?;
    let encoded = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    // Headerless compatibility is conservative: equal bytes in one activation reconcile.
    // Distinct deliberate identical calls require explicit IDs, stable across HTTP retries.
    let submission_id = call.key.unwrap_or_else(|| {
        format!(
            "protocol-{}",
            digest(
                &[
                    call.namespace
                        .as_deref()
                        .unwrap_or(&d.generation)
                        .as_bytes(),
                    &encoded
                ]
                .concat()
            )
        )
    });
    let workspace_scope = call
        .namespace
        .as_ref()
        .map(|namespace| crate::workspace_scope::key(&d.id, namespace));
    (crate::inference::Submission { service }).submit(
        &d.owner,
        Inference {
            schema: VERSION.into(),
            submission_id,
            reservation_id: d.id,
            generation: d.generation,
            profile_hash: d.profile_hash,
            prompt: String::new(),
            max_tokens,
            timeout_seconds: d.profile.inference_seconds,
            wait_seconds: None,
            workspace_scope,
            payload: Some(payload),
        },
    )
}
pub(crate) fn failure(error: String) -> axum::response::Response {
    let status = if error.starts_with("capacity_") {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::CONFLICT
    };
    (status, Json(json!({"schema":VERSION,"error":error}))).into_response()
}
