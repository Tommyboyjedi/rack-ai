use crate::{
    backend::BackendAccess,
    service::{Service, active, inflight, owns},
    types::*,
};
pub struct Dispatch<'a> {
    pub service: &'a Service,
}
impl Dispatch<'_> {
    pub fn run(&self, id: &str) -> Result<(), String> {
        let candidate = self.service.authority.read(|s| {
            let i = s.data.invocations.get(id).ok_or("missing_invocation")?;
            let d = s
                .data
                .demands
                .get(&i.request.reservation_id)
                .ok_or("missing_reservation")?;
            Ok((i.clone(), d.clone()))
        })?;
        if candidate.0.state != InvocationState::Accepted || candidate.1.state != DemandState::Ready
        {
            return Ok(());
        }
        (BackendAccess {
            config: &self.service.config,
        })
        .ready(&candidate.1)?;
        let started = self.service.authority.update(|s| {
            crate::workspace_scope::cancel_closed(s);
            let i = s
                .data
                .invocations
                .get(id)
                .ok_or("missing_invocation")?
                .clone();
            let d = s
                .data
                .demands
                .get(&i.request.reservation_id)
                .ok_or("missing_reservation")?
                .clone();
            if i.state != InvocationState::Accepted {
                return Ok(None);
            }
            if i.waiting_deadline <= now() || !active(&d) {
                s.data
                    .invocations
                    .get_mut(id)
                    .ok_or("missing_invocation")?
                    .state = InvocationState::Expired;
                return Ok(None);
            }
            if !crate::workers::eligible(s, &i)
                || !workspace_slot(self.service, s, &i)
                || d.state != DemandState::Ready
                || !owns(s, &d)
                || inflight(s, &d.id)
                || d.generation != candidate.1.generation
            {
                return Ok(None);
            }
            crate::idle::touch(s, &d.id, now())?;
            let i = s.data.invocations.get_mut(id).ok_or("missing_invocation")?;
            if crate::work_payload::is_workspace(i) {
                i.scope_access_hash = Some(digest(d.access_key.as_bytes()));
            }
            i.state = InvocationState::Started;
            i.started = Some(now());
            i.execution_deadline = i.started.map(|started| started + i.request.timeout_seconds);
            i.activation = Some(d.generation.clone());
            Ok(Some((i.clone(), d)))
        })?;
        let Some((invocation, demand)) = started else {
            return Ok(());
        };
        let result = if crate::work_payload::is_workspace(&invocation) {
            crate::work_execution::execute(self.service, (&demand, &invocation))
        } else {
            (BackendAccess {
                config: &self.service.config,
            })
            .infer(&demand, &invocation)
        };
        Completion {
            service: self.service,
        }
        .save((invocation, demand), result)
    }
}
struct Completion<'a> {
    service: &'a Service,
}
impl Completion<'_> {
    fn save(
        &self,
        context: (Invocation, Demand),
        result: Result<serde_json::Value, String>,
    ) -> Result<(), String> {
        let (invocation, demand) = context;
        let id = &invocation.id;
        self.service.authority.update(|s| {
            crate::workspace_scope::cancel_closed(s);
            let unknown_child = crate::work_payload::is_workspace(&invocation)
                && s.data.invocations.values().any(|child| {
                    child
                        .request
                        .workspace_scope
                        .as_ref()
                        .and_then(|scope| s.data.workspace_scopes.get(scope))
                        .is_some_and(|scope| scope.invocation_id.as_ref() == Some(id))
                        && matches!(
                            child.state,
                            InvocationState::Started | InvocationState::Uncertain
                        )
                });
            let i = s.data.invocations.get_mut(id).ok_or("missing_invocation")?;
            if i.state != InvocationState::Started || i.activation != invocation.activation {
                return Err("late_invocation_callback".into());
            }
            if unknown_child {
                i.state = InvocationState::Uncertain;
                i.error = Some("workspace_model_outcome_uncertain".into());
                i.late_result = result.ok();
                return Ok(());
            }
            match result {
                Ok(value) => {
                    let valid = (crate::work_payload::is_workspace(&invocation)
                        && value.get("packet_path").is_some())
                        || (invocation.request.payload.is_some()
                            && value.get("rack_protocol_response").is_some())
                        || value.get("model").and_then(serde_json::Value::as_str)
                            == Some(demand.profile.model.as_str())
                            && value
                                .get("choices")
                                .and_then(serde_json::Value::as_array)
                                .is_some_and(|v| !v.is_empty())
                            && value
                                .get("usage")
                                .and_then(|v| v.get("completion_tokens"))
                                .and_then(serde_json::Value::as_u64)
                                .is_some_and(|tokens| {
                                    tokens <= invocation.request.max_tokens as u64
                                });
                    if i.cancellation.is_some() {
                        i.late_result = Some(value);
                    } else {
                        i.result = Some(value);
                    }
                    i.state = if valid && i.cancellation.is_some() {
                        InvocationState::Cancelled
                    } else if valid {
                        InvocationState::Completed
                    } else {
                        InvocationState::Uncertain
                    };
                    if !valid {
                        i.error = Some("backend_result_identity_or_limits_unproven".into());
                    }
                }
                Err(e) => {
                    i.error = Some(crate::capacity::diagnostic(e));
                    i.state = InvocationState::Uncertain;
                }
            }
            if i.state != InvocationState::Uncertain {
                crate::idle::touch(s, &demand.id, now())?;
            }
            Ok(())
        })
    }
}

// The existing dispatch pool retains a free slot for a workspace's nested model call.
pub(crate) fn workspace_slot(service: &Service, s: &Document, i: &Invocation) -> bool {
    !crate::work_payload::is_workspace(i)
        || s.data
            .invocations
            .values()
            .filter(|other| {
                crate::work_payload::is_workspace(other) && other.state == InvocationState::Started
            })
            .count()
            < service.config.limits.max_dispatch_workers.saturating_sub(1)
}
