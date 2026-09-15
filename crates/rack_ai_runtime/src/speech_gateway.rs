use crate::scoped_gateway::failure;
use crate::{
    protocol::{Payload, Protocol},
    service::Service,
    types::*,
};
use axum::{
    Json,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::Arc;
pub async fn handle(
    service: Arc<Service>,
    call: (String, String, String, HeaderMap, Value),
) -> Response {
    let Ok(_permit) = service.gateway_waiters.clone().try_acquire_owned() else {
        return failure("capacity_gateway_waiters".into());
    };
    let copy = service.clone();
    let submitted = tokio::task::spawn_blocking(move || submit(&copy, call)).await;
    let invocation = match submitted {
        Ok(Ok(Reply::Voices(voices))) => return Json(json!({"voices":voices})).into_response(),
        Ok(Ok(Reply::Invocation(i))) => i,
        Ok(Err(e)) => return failure(e),
        Err(_) => return failure("receiver_failure".into()),
    };
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(crate::speech::MAX_SECONDS + 5);
    loop {
        let copy = service.clone();
        let owner = invocation.owner.clone();
        let id = invocation.id.clone();
        let result = tokio::task::spawn_blocking(move || {
            let i = copy.result(&owner, &id)?;
            if i.state == InvocationState::Completed {
                return crate::speech_backend::read(&copy.config.authority_root, &i).map(Some);
            }
            if matches!(
                i.state,
                InvocationState::Uncertain | InvocationState::Cancelled | InvocationState::Expired
            ) {
                return Err(format!("speech_{:?}:{}", i.state, i.id));
            }
            Ok(None)
        })
        .await;
        match result {
            Ok(Ok(Some(bytes))) => {
                return (
                    [("content-type", "audio/wav"), ("cache-control", "no-store")],
                    bytes,
                )
                    .into_response();
            }
            Ok(Ok(None)) => {}
            Ok(Err(e)) => return failure(e),
            Err(_) => return failure("receiver_failure".into()),
        }
        if std::time::Instant::now() >= deadline {
            return failure("speech_pending_reconcile_same_key".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
enum Reply {
    Voices(Vec<String>),
    Invocation(Box<Invocation>),
}
fn submit(
    service: &Service,
    call: (String, String, String, HeaderMap, Value),
) -> Result<Reply, String> {
    let (id, capability, path, headers, body) = call;
    let source = service.config.authenticate(
        headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .unwrap_or(""),
    )?;
    let d = service.authority.read(|s| {
        let d = crate::service::owned(s, &source.source, &id)?;
        use subtle::ConstantTimeEq;
        if !bool::from(d.access_key.as_bytes().ct_eq(capability.as_bytes()))
            || !crate::service::active(d)
            || d.state != DemandState::Ready
            || !crate::service::owns(s, d)
            || d.profile.backend != crate::config::Backend::Chatterbox
        {
            return Err("invalid_scoped_capability".into());
        }
        Ok(d.clone())
    })?;
    let voices = crate::speech_backend::health(&d)?.voices;
    if path == "voices" {
        if body != json!({}) {
            return Err("invalid_voice_discovery".into());
        }
        return Ok(Reply::Voices(voices));
    }
    let key = headers
        .get("idempotency-key")
        .and_then(|h| h.to_str().ok())
        .filter(|v| valid_id(v))
        .ok_or("speech_idempotency_key_required")?
        .to_string();
    let payload = Payload {
        protocol: Protocol::Speech,
        body,
    };
    crate::speech::validate(&payload, &d)?;
    let voice = payload.body["voice"].as_str().ok_or("invalid_voice_id")?;
    if !voices.iter().any(|v| v == voice) {
        return Err("unknown_voice_id".into());
    }
    (crate::inference::Submission { service })
        .submit(
            &d.owner,
            Inference {
                schema: VERSION.into(),
                submission_id: key,
                reservation_id: d.id,
                generation: d.generation,
                profile_hash: d.profile_hash,
                prompt: String::new(),
                payload: Some(payload),
                max_tokens: 1,
                timeout_seconds: d.profile.inference_seconds,
                wait_seconds: Some(1),
                workspace_scope: None,
            },
        )
        .map(|i| Reply::Invocation(Box::new(i)))
}
