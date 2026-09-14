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
            if let Some(i) = s.data.invocations.values().find(|i| i.owner == owner && i.request.submission_id == request.submission_id) {
                return if i.request == request { Ok(i.clone()) } else { Err("identity_conflict".into()) };
            }
            let d = owned(s, owner, &request.reservation_id)?;
            if d.profile.backend == crate::config::Backend::Comfyui { return Err("use_versioned_media_interface".into()); }
            if !active(d) || !matches!(d.state, DemandState::Ready | DemandState::Held | DemandState::Preparing) { return Err("reservation_not_dispatchable".into()); }
            if d.generation != request.generation || d.profile_hash != request.profile_hash { return Err("stale_generation_or_profile".into()); }
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
            let invocation = Invocation { id: identity()?, owner: owner.into(), deadline: now() + request.timeout_seconds,
                request, state: InvocationState::Accepted, started: None, activation: None, result: None, error: None };
            s.data.invocations.insert(invocation.id.clone(), invocation.clone());
            Ok(invocation)
        })
    }
}
