use crate::{
    service::{Service, active, owned},
    types::*,
};
pub struct Submission<'a> {
    pub service: &'a Service,
}
impl Submission<'_> {
    pub fn submit(&self, owner: &str, request: Inference) -> Result<Invocation, String> {
        self.service.authority.update(|s| {
            if let Some(i) = s.data.invocations.values().find(|i| i.owner == owner && i.request.reservation_id == request.reservation_id && i.request.submission_id == request.submission_id) {
                let mut retry = request.clone();
                if retry.generation != i.request.generation {
                    let current = owned(s, owner, &request.reservation_id)?;
                    if !active(current) || current.generation != retry.generation { return Err("stale_generation_or_profile".into()); }
                    retry.generation = i.request.generation.clone();
                }
                return if i.request == retry { Ok(i.clone()) } else { Err("identity_conflict".into()) };
            }
            if !crate::workspace_scope::permits(s, &request) { return Err("workspace_scope_closed_or_unknown".into()); }
            let d = owned(s, owner, &request.reservation_id)?;
            if d.profile.backend == crate::config::Backend::Comfyui { return Err("use_versioned_media_interface".into()); }
            if !active(d) || !matches!(d.state, DemandState::Ready | DemandState::Held | DemandState::Preparing | DemandState::Draining) { return Err("reservation_not_dispatchable".into()); }
            if d.generation != request.generation || d.profile_hash != request.profile_hash { return Err("stale_generation_or_profile".into()); }
            if d.profile.backend == crate::config::Backend::Chatterbox {
                crate::speech::admit(s, (d, &request))?;
            }
            let input_bytes = if let Some(payload) = &request.payload {
                if payload.validate(d)? != request.max_tokens { return Err("output_limit_mismatch".into()); }
                serde_json::to_vec(payload).map_err(|e| e.to_string())?.len()
            } else { request.prompt.len() };
            if request.schema != VERSION || !valid_id(&request.submission_id) || request.timeout_seconds == 0
                || request.timeout_seconds > d.profile.inference_seconds || request.max_tokens == 0
                || request.max_tokens > d.profile.max_output_tokens
                // UTF-8 byte count is a conservative token bound for qualified byte-fallback tokenizers.
                || input_bytes as u64 + request.max_tokens as u64 > d.request.context_tokens as u64 {
                return Err("inference_limits".into());
            }
            let wait = request.wait_seconds.unwrap_or(self.service.config.limits.max_wait_seconds);
            if wait == 0 || wait > self.service.config.limits.max_wait_seconds { return Err("waiting_limits".into()); }
            self.service.config.limits.pending(s, &request.reservation_id)?;
            let invocation = Invocation { id: identity()?, owner: owner.into(), waiting_deadline: now() + wait, execution_deadline: None,
                response_bytes: if request.payload.as_ref().is_some_and(|p| p.protocol == crate::protocol::Protocol::Speech) { crate::speech::MAX_WAV_BYTES } else { self.service.config.limits.max_response_bytes }, cancellation: None, late_result: None,
                request, state: InvocationState::Accepted, started: None, activation: None, result: None, error: None };
            crate::idle::touch(s, &invocation.request.reservation_id, now())?;
            s.data.invocations.insert(invocation.id.clone(), invocation.clone());
            crate::capacity::retention(s, &self.service.config.limits)?;
            Ok(invocation)
        })
    }
}
