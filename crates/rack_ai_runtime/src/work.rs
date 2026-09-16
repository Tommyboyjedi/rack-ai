use crate::{
    service::Service,
    types::*,
    work_payload::{Payload, Work},
};
use serde_json::{Value, json};
pub struct WorkSubmission<'a> {
    pub service: &'a Service,
}
impl WorkSubmission<'_> {
    pub fn submit(&self, input: (&str, Work)) -> Result<Invocation, String> {
        let (owner, work) = input;
        if !valid_id(&work.work_id) {
            return Err("invalid_work_id".into());
        }
        if let Some(i) = self
            .service
            .authority
            .read(|s| Ok(find(s, owner, &work.work_id).cloned()))?
        {
            return if i.request.work.as_ref() == Some(&work) {
                Ok(i)
            } else {
                Err("identity_conflict".into())
            };
        }
        let d = self.service.authority.read(|s| {
            crate::reservation::select(s, (owner, &work.reservation_id, &work.service)).cloned()
        })?;
        if d.state == DemandState::Denied {
            return Err("service_unavailable".into());
        }
        let (prompt, max_tokens, timeout_seconds) = match &work.payload {
            Payload::Inference {
                prompt,
                max_tokens,
                timeout_seconds,
            } => (prompt.clone(), *max_tokens, *timeout_seconds),
            Payload::Workspace { workspace } => {
                crate::work_execution::selection(self.service, (&d, &work))?;
                (
                    String::new(),
                    1,
                    u64::from(workspace.limits.timeout_seconds),
                )
            }
        };
        (crate::inference::Submission {
            service: self.service,
        })
        .submit(
            owner,
            Inference {
                schema: VERSION.into(),
                submission_id: format!("work-{}", digest(work.work_id.as_bytes())),
                reservation_id: d.id,
                generation: d.generation,
                profile_hash: d.profile_hash,
                prompt,
                payload: None,
                max_tokens,
                timeout_seconds,
                wait_seconds: work.wait_seconds,
                workspace_scope: None,
                work: Some(work),
            },
        )
    }
}
pub fn find<'a>(s: &'a Document, owner: &str, id: &str) -> Option<&'a Invocation> {
    s.data
        .invocations
        .values()
        .find(|i| i.owner == owner && i.request.work.as_ref().is_some_and(|w| w.work_id == id))
}
pub fn inspect(service: &Service, input: (&str, &str)) -> Result<Value, String> {
    service.authority.read(|s| {
        let i = find(s, input.0, input.1).ok_or("not_found")?;
        let w = i.request.work.as_ref().ok_or("work_required")?;
        let d = crate::service::owned(s, input.0, &i.request.reservation_id)?;
        let state = if (i.state == InvocationState::Accepted || (crate::work_payload::is_workspace(i) && i.state == InvocationState::Started)) && !crate::reservation::ready(s, d) {
            if matches!(d.state, DemandState::Held | DemandState::Draining) { "held".into() } else { "waiting".into() }
        } else { serde_json::to_value(i.state).map_err(|e| e.to_string())? };
        Ok(json!({"work_id":w.work_id,"reservation_id":w.reservation_id,"service":w.service,"state":state,
            "invocation_id":i.id,"started":i.started,"activation":i.activation,"result":i.result,
            "late_result":i.late_result,"cancellation":i.cancellation,"error":i.error}))
    })
}
pub fn cancel(service: &Service, input: (&str, &str)) -> Result<Value, String> {
    let id = service.authority.read(|s| {
        find(s, input.0, input.1)
            .map(|i| i.id.clone())
            .ok_or("not_found".into())
    })?;
    (crate::control::ReservationControl { service }).cancel_invocation(input.0, &id)?;
    inspect(service, input)
}
