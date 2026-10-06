//! Best-effort capture at the effective backend request boundary.
use crate::{
    interaction_diagnostics::{self as diagnostics, CallIdentity, METADATA_BYTES},
    interaction_redaction::Redactor,
    interaction_store::{self as store, Content, End, Lease, Start},
    service::Service,
    types::*,
};
use serde_json::Value;

pub struct Capture<'a> {
    service: &'a Service,
    owner: String,
    identity: CallIdentity,
    redactor: Redactor,
    start: Start,
}
pub fn begin<'a>(
    service: &'a Service,
    input: (&Demand, &Invocation, &Value),
) -> Option<Capture<'a>> {
    match prepare(service, input) {
        Ok(capture) => capture,
        Err(error) => {
            eprintln!(
                "interaction diagnostics capture unavailable: {}",
                crate::capacity::diagnostic(error)
            );
            None
        }
    }
}
fn prepare<'a>(
    service: &'a Service,
    input: (&Demand, &Invocation, &Value),
) -> Result<Option<Capture<'a>>, String> {
    let (d, i, effective) = input;
    let Some(identity) = diagnostics::identity(service, (d, i))? else {
        return Ok(None);
    };
    let _lock = store::lock(service)?;
    if !service
        .authority
        .read(|s| Ok(diagnostics::live(s, (&i.owner, &identity.reservation_id))))?
    {
        return Ok(None);
    }
    let dir = store::directory(service, (&i.owner, &identity.reservation_id));
    store::private_directory(&dir)?;
    let mut lease = if dir.join("lease.json").exists() {
        store::lease(&dir)?
    } else {
        Lease {
            owner: i.owner.clone(),
            reservation_id: identity.reservation_id.clone(),
            limits: service.config.interaction_diagnostics.clone(),
            omitted_interactions: 0,
            cleanup_pending: false,
        }
    };
    if lease.cleanup_pending {
        return Ok(None);
    }
    store::save_lease(&dir, &lease)?;
    let calls = store::starts(&dir, lease.limits.max_interactions_per_reservation)?;
    let work_count = calls
        .iter()
        .filter(|c| c.identity.work_id == identity.work_id)
        .count();
    if calls.len() >= lease.limits.max_interactions_per_reservation
        || work_count >= lease.limits.max_interactions_per_work
    {
        lease.omitted_interactions = lease.omitted_interactions.saturating_add(1);
        store::save_lease(&dir, &lease)?;
        return Ok(None);
    }
    let charged = calls.iter().map(|c| c.charged_content_bytes).sum::<u64>();
    let metadata = (lease.limits.max_interactions_per_reservation as u64 + 1) * METADATA_BYTES;
    let available = lease
        .limits
        .max_reservation_bytes
        .saturating_sub(metadata + charged);
    let request_bound = lease.limits.max_request_bytes.min(available / 2);
    let response_bound = lease
        .limits
        .max_response_bytes
        .min(available.saturating_sub(request_bound));
    let redactor = service
        .authority
        .read(|s| Ok(Redactor::new(&service.config, s)))?;
    let start = Start {
        identity: identity.clone(),
        artifact_id: store::artifact_id(&identity, &i.owner),
        dispatched_ms: crate::workspace_scope::now_ms(),
        request: Content::bounded(redactor.value(effective), request_bound)?,
        response_bound,
        charged_content_bytes: request_bound + response_bound,
    };
    let path = store::document_path(&dir, &start.artifact_id, "start")?;
    if path.exists() {
        return Err("duplicate_diagnostic_call".into());
    }
    store::write(&path, &start)?;
    Ok(Some(Capture {
        service,
        owner: i.owner.clone(),
        identity,
        redactor,
        start,
    }))
}
impl Capture<'_> {
    pub fn dispatched(&mut self) {
        self.start.dispatched_ms = crate::workspace_scope::now_ms();
    }
    pub fn finish(self, input: (Option<Value>, &Result<Value, String>)) {
        if let Err(error) = self.persist_end(input) {
            eprintln!(
                "interaction diagnostics completion unavailable: {}",
                crate::capacity::diagnostic(error)
            );
        }
    }
    fn persist_end(&self, input: (Option<Value>, &Result<Value, String>)) -> Result<(), String> {
        let completed_ms = crate::workspace_scope::now_ms();
        let _lock = store::lock(self.service)?;
        if !self.service.authority.read(|s| {
            Ok(diagnostics::live(
                s,
                (&self.owner, &self.identity.reservation_id),
            ))
        })? {
            return Ok(());
        }
        let dir = store::directory(self.service, (&self.owner, &self.identity.reservation_id));
        if !dir.exists() || store::lease(&dir)?.cleanup_pending {
            return Ok(());
        }
        let mut end = End {
            completed_ms,
            dispatched_ms: self.start.dispatched_ms,
            response: input
                .0
                .map(|v| Content::bounded(self.redactor.value(&v), self.start.response_bound))
                .transpose()?,
            error: input.1.as_ref().err().map(|e| self.redactor.text(e)),
            terminal_state: Some(match input.1 {
                Ok(_) => InvocationState::Completed,
                Err(error) => crate::dispatch::failure_state(error),
            }),
        };
        if input.1.as_ref().err().is_some_and(|error| {
            matches!(
                error.as_str(),
                "backend_response_oversized"
                    | "backend_read_uncertain"
                    | "invalid_protocol_encoding"
            )
        }) {
            if let Some(response) = end.response.as_mut() {
                response.truncated = true;
                response.unavailable_reason = Some("backend_response_incomplete".into());
            }
        }
        let path = store::document_path(&dir, &self.start.artifact_id, "end")?;
        if path.exists() {
            return Err("duplicate_diagnostic_completion".into());
        }
        store::write(&path, &end)
    }
}
