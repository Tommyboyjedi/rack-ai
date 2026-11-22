//! Capability URLs rotate with activation. Explicit keys identify logical calls within a reservation.
use crate::{protocol::Protocol, service::Service, types::*};
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
    if path == "speech" || path == "voices" {
        return crate::speech_gateway::handle(service, (id, generation, path, headers, body)).await;
    }
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
    let timeout = match headers.get("rackai-execution-timeout-seconds") {
        None => None,
        Some(header) => match header
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
        {
            Some(value) if value > 0 => Some(value),
            _ => return failure("invalid_execution_timeout".into()),
        },
    };
    let report_progress = headers.get("prefer").is_some_and(|value| {
        value.to_str().is_ok_and(|value| {
            value
                .split(',')
                .any(|pref| pref.trim().eq_ignore_ascii_case("respond-async"))
        })
    });
    if report_progress && (key.is_none() || namespace.is_some()) {
        return failure("async_requires_idempotency_key".into());
    }
    let priority = interactive_priority(&headers);
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
                timeout,
                priority,
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
        if report_progress {
            return current_response(&service, &result);
        }
        if let Some(response) = terminal_response(&service, &result) {
            return response;
        }
        if std::time::Instant::now() >= deadline {
            return crate::service_error::wait_expired(&service, &result);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
fn current_response(service: &Service, result: &Invocation) -> axum::response::Response {
    terminal_response(service, result).unwrap_or_else(|| progress_response(result))
}

fn terminal_response(service: &Service, result: &Invocation) -> Option<axum::response::Response> {
    if result.state == InvocationState::Completed {
        let Some(value) = &result.result else {
            return Some(failure("missing_protocol_result".into()));
        };
        let response = &value["rack_protocol_response"];
        let content_type = response["content_type"]
            .as_str()
            .unwrap_or("application/json")
            .to_string();
        return Some(
            (
                [("content-type", content_type)],
                response["body"].as_str().unwrap_or("").to_string(),
            )
                .into_response(),
        );
    }
    if matches!(
        result.state,
        InvocationState::Failed
            | InvocationState::Uncertain
            | InvocationState::Cancelled
            | InvocationState::Expired
    ) {
        return Some(crate::service_error::invocation(service, result));
    }
    None
}

fn progress_response(result: &Invocation) -> axum::response::Response {
    (
        StatusCode::ACCEPTED,
        [
            ("retry-after", "1"),
            ("preference-applied", "respond-async"),
        ],
        Json(json!({
            "schema": VERSION,
            "invocation_id": result.id,
            "state": result.state,
            "queue_deadline": result.waiting_deadline,
            "execution_deadline": result.execution_deadline,
        })),
    )
        .into_response()
}

pub async fn status(
    State(service): State<Arc<Service>>,
    Path((reservation, capability, invocation)): Path<(String, String, String)>,
) -> axum::response::Response {
    let copy = Arc::clone(&service);
    let result = tokio::task::spawn_blocking(move || {
        use subtle::ConstantTimeEq;
        let owner = copy.authority.read(|s| {
            s.data
                .demands
                .get(&reservation)
                .filter(|d| bool::from(d.access_key.as_bytes().ct_eq(capability.as_bytes())))
                .map(|d| d.owner.clone())
                .ok_or_else(|| "invalid_scoped_capability".to_string())
        })?;
        let result = copy.result(&owner, &invocation)?;
        if result.request.reservation_id != reservation || result.request.workspace_scope.is_some()
        {
            return Err("invalid_scoped_capability".into());
        }
        Ok(result)
    })
    .await;
    match result {
        Ok(Ok(result)) => current_response(&service, &result),
        Ok(Err(error)) => failure(error),
        Err(_) => failure("receiver_failure".into()),
    }
}

fn interactive_priority(headers: &HeaderMap) -> InvocationPriority {
    let Ok(secret) = std::env::var("RACKAI_INTERACTIVE_QUEUE_KEY") else {
        return InvocationPriority::Background;
    };
    priority_for(headers, &secret)
}

fn priority_for(headers: &HeaderMap, secret: &str) -> InvocationPriority {
    use subtle::ConstantTimeEq;
    if !secret.is_empty()
        && headers
            .get("rackai-interactive-queue-key")
            .is_some_and(|header| bool::from(secret.as_bytes().ct_eq(header.as_bytes())))
    {
        InvocationPriority::Interactive
    } else {
        InvocationPriority::Background
    }
}

struct GatewayCall {
    id: String,
    generation: String,
    protocol: Protocol,
    body: Value,
    key: Option<String>,
    namespace: Option<String>,
    timeout: Option<u64>,
    priority: InvocationPriority,
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
            && !crate::workspace_scope::authorizes(
                s,
                (&d.id, &call.generation, call.namespace.as_deref()),
            )
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
            Protocol::Speech => return Err("use_speech_interface".into()),
        };
        object.insert(key.into(), json!(d.profile.max_output_tokens));
    }
    let payload = crate::service_request::normalize(crate::service_request::NormalizeInput {
        profile: &d.profile,
        protocol: call.protocol,
        body,
    })?;
    let max_tokens = payload.validate(&d)?;
    let timeout_seconds = call.timeout.unwrap_or(d.profile.inference_seconds);
    if timeout_seconds > d.profile.inference_seconds {
        return Err("invalid_execution_timeout".into());
    }
    let mut encoded = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    if let Some(timeout) = call.timeout {
        encoded.extend_from_slice(&timeout.to_be_bytes());
    }
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
    (crate::inference::Submission { service }).submit_with_priority(
        &d.owner,
        Inference {
            work: None,
            schema: VERSION.into(),
            submission_id,
            reservation_id: d.id,
            generation: d.generation,
            profile_hash: d.profile_hash,
            prompt: String::new(),
            max_tokens,
            timeout_seconds,
            wait_seconds: None,
            workspace_scope,
            payload: Some(payload),
        },
        call.priority,
    )
}
pub(crate) fn failure(error: String) -> axum::response::Response {
    crate::service_error::generic(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_trusted_ingress_can_mark_requests_interactive() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            priority_for(&headers, "fixture-key"),
            InvocationPriority::Background
        );
        headers.insert("rackai-interactive-queue-key", "wrong".parse().unwrap());
        assert_eq!(
            priority_for(&headers, "fixture-key"),
            InvocationPriority::Background
        );
        headers.insert(
            "rackai-interactive-queue-key",
            "fixture-key".parse().unwrap(),
        );
        assert_eq!(priority_for(&headers, ""), InvocationPriority::Background);
        assert_eq!(
            priority_for(&headers, "fixture-key"),
            InvocationPriority::Interactive
        );
    }
}
