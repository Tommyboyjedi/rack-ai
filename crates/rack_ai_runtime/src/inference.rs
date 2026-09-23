use crate::{
    service::{Service, active, owned},
    types::*,
};
pub struct Submission<'a> {
    pub service: &'a Service,
}
impl Submission<'_> {
    pub fn submit(&self, owner: &str, request: Inference) -> Result<Invocation, String> {
        let root = self.service.config.authority_root.clone();
        let invocation = self.service.authority.update(|s| {
            let archived_identity_lookup_needed = owned(s, owner, &request.reservation_id)
                .ok()
                .filter(|d| active(d))
                .is_none_or(|d| d.accepted_calls > 0);
            if let Some(work) = &request.work {
                if let Some(i) = crate::work::find(s, owner, &work.work_id) {
                    return if work_matches(i, work)? {
                        Ok(i.clone())
                    } else {
                        Err("identity_conflict".into())
                    };
                }
                if archived_identity_lookup_needed {
                    if let Some(i) = crate::history_archive::lookup_work(
                        &self.service.config.authority_root,
                        owner,
                        &work.work_id,
                    )? {
                        return if work_matches(&i, work)? {
                            Ok(i)
                        } else {
                            Err("identity_conflict".into())
                        };
                    }
                }
                let selected =
                    crate::reservation::select(s, (owner, &work.reservation_id, &work.service))?;
                if selected.id != request.reservation_id {
                    return Err("service_not_reserved".into());
                }
            }
            if let Some(i) = s.data.invocations.values().find(|i| {
                i.owner == owner
                    && i.request.reservation_id == request.reservation_id
                    && i.request.submission_id == request.submission_id
            }) {
                let mut retry = request.clone();
                if retry.generation != i.request.generation {
                    let current = owned(s, owner, &request.reservation_id)?;
                    if !active(current) || current.generation != retry.generation {
                        return Err("stale_generation_or_profile".into());
                    }
                    retry.generation = i.request.generation.clone();
                }
                return if request_matches(i, &retry)? {
                    Ok(i.clone())
                } else {
                    Err("identity_conflict".into())
                };
            }
            if archived_identity_lookup_needed {
                if let Some(i) = crate::history_archive::lookup_submission(
                    &self.service.config.authority_root,
                    owner,
                    &request.reservation_id,
                    &request.submission_id,
                )? {
                    return if request_matches(&i, &request)? {
                        Ok(i)
                    } else {
                        Err("identity_conflict".into())
                    };
                }
            }
            if !crate::workspace_scope::permits(s, &request) {
                return Err("workspace_scope_closed_or_unknown".into());
            }
            let d = owned(s, owner, &request.reservation_id)?;
            if d.profile.backend == crate::config::Backend::Comfyui {
                return Err("use_versioned_media_interface".into());
            }
            if d.state == DemandState::Preempting {
                return Err("reservation_preempting".into());
            }
            if d.state == DemandState::Preempted {
                return Err("reservation_preempted".into());
            }
            if d.state == DemandState::Unavailable {
                return Err("service_unavailable".into());
            }
            if !active(d) || d.state != DemandState::Ready || !crate::reservation::ready(s, d) {
                return Err("reservation_not_dispatchable".into());
            }
            if d.generation != request.generation || d.profile_hash != request.profile_hash {
                return Err("stale_generation_or_profile".into());
            }
            if d.profile.backend == crate::config::Backend::Chatterbox {
                crate::speech::admit(s, (d, &request))?;
            }
            let workspace = request
                .work
                .as_ref()
                .is_some_and(|w| w.workspace().is_some());
            if let Some(payload) = &request.payload {
                if payload.validate(d)? != request.max_tokens {
                    return Err("output_limit_mismatch".into());
                }
            }
            if request.schema != VERSION
                || !valid_id(&request.submission_id)
                || request.timeout_seconds == 0
                || request.timeout_seconds
                    > if workspace {
                        crate::work_execution::MAX_WORKSPACE_SECONDS
                    } else {
                        d.profile.inference_seconds
                    }
                || request.max_tokens == 0
                || request.max_tokens > d.profile.max_output_tokens
            {
                return Err("inference_limits".into());
            }
            let wait = request
                .wait_seconds
                .unwrap_or(self.service.config.limits.max_wait_seconds);
            if wait == 0 || wait > self.service.config.limits.max_wait_seconds {
                return Err("waiting_limits".into());
            }
            self.service
                .config
                .limits
                .pending(s, &request.reservation_id)?;
            let accepted_calls = s
                .data
                .demands
                .get(&request.reservation_id)
                .ok_or("missing_reservation")?
                .accepted_calls;
            s.data
                .demands
                .get_mut(&request.reservation_id)
                .ok_or("missing_reservation")?
                .accepted_calls = accepted_calls.saturating_add(1);
            let queue_order = s.data.next_invocation_order;
            s.data.next_invocation_order = s.data.next_invocation_order.saturating_add(1);
            let response_bytes = if workspace {
                crate::work_execution::WORKSPACE_RESPONSE_BYTES
            } else if request
                .payload
                .as_ref()
                .is_some_and(|p| p.protocol == crate::protocol::Protocol::Speech)
            {
                crate::speech::MAX_WAV_BYTES
            } else {
                self.service.config.limits.max_response_bytes
            };
            let mut invocation = Invocation {
                scope_access_hash: None,
                id: identity()?,
                owner: owner.into(),
                request,
                request_digest: None,
                request_bytes: None,
                work_digest: None,
                work_bytes: None,
                request_ref: None,
                state: InvocationState::Queued,
                queue_order,
                waiting_deadline: now() + wait,
                execution_deadline: None,
                response_bytes,
                cancellation: None,
                late_result: None,
                result_digest: None,
                late_result_digest: None,
                result_ref: None,
                late_result_ref: None,
                started: None,
                activation: None,
                result: None,
                error: None,
            };
            crate::payload_store::store_request(&root, &mut invocation)?;
            let admitted = (|| {
                crate::idle::touch(s, &invocation.request.reservation_id, now())?;
                crate::activity_retention::refresh(
                    s,
                    (self.service.config.idle_timeout_seconds, now()),
                )?;
                s.data
                    .invocations
                    .insert(invocation.id.clone(), invocation.clone());
                crate::capacity::retention(s, &self.service.config.limits)?;
                Ok(invocation.clone())
            })();
            if admitted.is_err() {
                let _ = crate::payload_store::remove_invocation_payloads(&root, &invocation);
            }
            admitted
        })?;
        crate::payload_store::hydrate_invocation(&root, invocation)
    }
}

pub(crate) fn request_matches(
    invocation: &Invocation,
    request: &Inference,
) -> Result<bool, String> {
    if let Some(digest) = &invocation.request_digest {
        return Ok(&json_digest(request)? == digest);
    }
    Ok(invocation.request == *request)
}

pub(crate) fn work_matches(
    invocation: &Invocation,
    work: &crate::work_payload::Work,
) -> Result<bool, String> {
    if let Some(digest) = &invocation.work_digest {
        return Ok(&json_digest(work)? == digest);
    }
    Ok(invocation.request.work.as_ref() == Some(work))
}
