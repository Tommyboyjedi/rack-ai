//! Capability URLs are minted only after authenticated acquisition. Generation rotation revokes them.
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
    let protocol = match path.as_str() {
        "chat/completions" => Protocol::ChatCompletions,
        "responses" => Protocol::Responses,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let key = headers
        .get("idempotency-key")
        .and_then(|h| h.to_str().ok())
        .map(String::from);
    let response = tokio::task::spawn_blocking(move || {
        let d = service.authority.update(|s| {
            let d = s.data.demands.get(&id).ok_or("invalid_scoped_capability")?;
            use subtle::ConstantTimeEq;
            if !bool::from(d.access_key.as_bytes().ct_eq(generation.as_bytes()))
                || !crate::service::active(d)
            {
                return Err("invalid_scoped_capability".into());
            }
            Ok(d.clone())
        })?;
        let mut body = body;
        let object = body.as_object_mut().ok_or("invalid_protocol_body")?;
        if !["max_tokens", "max_completion_tokens", "max_output_tokens"]
            .iter()
            .any(|k| object.contains_key(*k))
        {
            let key = match protocol {
                Protocol::ChatCompletions => "max_tokens",
                Protocol::Responses => "max_output_tokens",
            };
            object.insert(key.into(), json!(d.profile.max_output_tokens));
        }
        let payload = Payload { protocol, body };
        let max_tokens = payload.validate(&d)?;
        let encoded = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
        let invocation = (crate::inference::Submission { service: &service }).submit(
            &d.owner,
            Inference {
                schema: VERSION.into(),
                submission_id: key.unwrap_or_else(|| format!("protocol-{}", digest(&encoded))),
                reservation_id: d.id.clone(),
                generation: d.generation.clone(),
                profile_hash: d.profile_hash.clone(),
                prompt: String::new(),
                max_tokens,
                timeout_seconds: d.profile.inference_seconds,
                payload: Some(payload),
            },
        )?;
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(d.profile.inference_seconds + 1);
        loop {
            let result = service.result(&d.owner, &invocation.id)?;
            if result.state == InvocationState::Completed {
                return result.result.ok_or("missing_protocol_result".into());
            }
            if result.state == InvocationState::Uncertain
                || result.state == InvocationState::Cancelled
                || result.state == InvocationState::Expired
                || std::time::Instant::now() >= deadline
            {
                return Err(format!("invocation_{:?}:{}", result.state, result.id));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    })
    .await;
    match response {
        Ok(Ok(value)) => {
            let response = &value["rack_protocol_response"];
            let content_type = response["content_type"]
                .as_str()
                .unwrap_or("application/json")
                .to_string();
            let body = response["body"].as_str().unwrap_or("").to_string();
            ([("content-type", content_type)], body).into_response()
        }
        Ok(Err(error)) => (
            StatusCode::CONFLICT,
            Json(json!({"schema":VERSION,"error":error})),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
