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
        if candidate.0.state != InvocationState::Queued || candidate.1.state != DemandState::Ready {
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
            if i.state != InvocationState::Queued {
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
            crate::activity_retention::refresh(
                s,
                (self.service.config.idle_timeout_seconds, now()),
            )?;
            let i = s.data.invocations.get_mut(id).ok_or("missing_invocation")?;
            if crate::work_payload::is_workspace(i) {
                i.scope_access_hash = Some(digest(d.access_key.as_bytes()));
            }
            i.state = InvocationState::Running;
            i.started = Some(now());
            i.execution_deadline = i.started.map(|started| started + i.request.timeout_seconds);
            i.activation = Some(d.generation.clone());
            Ok(Some((i.clone(), d)))
        })?;
        let Some((invocation, demand)) = started else {
            return Ok(());
        };
        let invocation = crate::payload_store::hydrate_invocation(
            &self.service.config.authority_root,
            invocation,
        )?;
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
#[cfg(test)]
static FAIL_COMPLETION_AFTER_PAYLOAD: std::sync::Mutex<Option<String>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
fn fail_completion_after_payload_for(invocation_id: &str) {
    *FAIL_COMPLETION_AFTER_PAYLOAD.lock().unwrap() = Some(invocation_id.to_string());
}

impl Completion<'_> {
    fn save(
        &self,
        context: (Invocation, Demand),
        result: Result<serde_json::Value, String>,
    ) -> Result<(), String> {
        let (invocation, demand) = context;
        let id = &invocation.id;
        let saved = self.service.authority.update(|s| {
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
                            InvocationState::Running | InvocationState::Uncertain
                        )
                });
            let i = s.data.invocations.get_mut(id).ok_or("missing_invocation")?;
            if i.state != InvocationState::Running || i.activation != invocation.activation {
                return Err("late_invocation_callback".into());
            }
            if unknown_child {
                i.state = InvocationState::Uncertain;
                i.error = Some("workspace_model_outcome_uncertain".into());
                if let Ok(value) = result {
                    crate::payload_store::store_late_result(
                        &self.service.config.authority_root,
                        i,
                        value,
                    )?;
                }
                return Ok(());
            }
            let previous_result_ref = i.result_ref.clone();
            let previous_late_result_ref = i.late_result_ref.clone();
            let mut stored_payload = false;
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
                        crate::payload_store::store_late_result(
                            &self.service.config.authority_root,
                            i,
                            value,
                        )?;
                    } else {
                        crate::payload_store::store_result(
                            &self.service.config.authority_root,
                            i,
                            value,
                        )?;
                    }
                    stored_payload = true;
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
                    i.state = failure_state(&e);
                    i.error = Some(crate::capacity::diagnostic(e));
                }
            }
            let terminal_uncertain = i.state == InvocationState::Uncertain;
            let cleanup_invocation = stored_payload.then(|| i.clone());
            let _ = i;
            let finalized = (|| {
                #[cfg(test)]
                {
                    let mut fail_after_payload = FAIL_COMPLETION_AFTER_PAYLOAD.lock().unwrap();
                    if fail_after_payload.as_deref() == Some(id) {
                        *fail_after_payload = None;
                        return Err("injected_completion_finalization_failure".into());
                    }
                }
                if !terminal_uncertain {
                    crate::idle::touch(s, &demand.id, now())?;
                    crate::activity_retention::refresh(
                        s,
                        (self.service.config.idle_timeout_seconds, now()),
                    )?;
                }
                crate::capacity::retention(s, &self.service.config.limits)?;
                Ok(())
            })();
            if finalized.is_err() {
                if let Some(invocation) = cleanup_invocation.as_ref() {
                    let _ = crate::payload_store::remove_uncommitted_result_payloads(
                        &self.service.config.authority_root,
                        invocation,
                        previous_result_ref.as_ref(),
                        previous_late_result_ref.as_ref(),
                    );
                }
            }
            finalized
        });
        if saved.is_ok() {
            self.service.request_history_maintenance();
        }
        saved
    }
}

fn failure_state(error: &str) -> InvocationState {
    match error {
        "backend_response_oversized" => InvocationState::Failed,
        "backend_transport_uncertain"
        | "backend_read_uncertain"
        | "speech_transport_uncertain"
        | "speech_read_uncertain" => InvocationState::Uncertain,
        _ => InvocationState::Uncertain,
    }
}

// The existing dispatch pool retains a free slot for a workspace's nested model call.
pub(crate) fn workspace_slot(service: &Service, s: &Document, i: &Invocation) -> bool {
    !crate::work_payload::is_workspace(i)
        || s.data
            .invocations
            .values()
            .filter(|other| {
                crate::work_payload::is_workspace(other) && other.state == InvocationState::Running
            })
            .count()
            < service.config.limits.max_dispatch_workers.saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        admission::Admission,
        config::{Config, Driver},
    };
    use serde_json::json;
    use std::fs;

    struct Fixture {
        service: std::sync::Arc<Service>,
    }

    impl Fixture {
        fn new() -> Self {
            let mut config: Config =
                serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                    .unwrap();
            config.authority_root =
                std::env::temp_dir().join(format!("rack-dispatch-{}", identity().unwrap()));
            fs::create_dir_all(&config.authority_root).unwrap();
            config.fixture_mode = true;
            for profile in &mut config.profiles {
                profile.driver = Driver::Fixture;
                profile.qualified = true;
                profile.evidence = vec!["synthetic-no-gpu".into()];
            }
            Self {
                service: std::sync::Arc::new(Service::new(config)),
            }
        }

        fn acquire_ready(&self) -> Demand {
            let source = self
                .service
                .config
                .sources
                .iter()
                .find(|source| source.source == "athba")
                .unwrap();
            let profile = self
                .service
                .config
                .profiles
                .iter()
                .find(|profile| profile.tag == "local-primary")
                .unwrap();
            let request = Acquire {
                schema: VERSION.into(),
                source_system: "athba".into(),
                work_id: "completion-rollback".into(),
                acquisition_id: identity().unwrap(),
                tag: "local-primary".into(),
                priority: Some(Priority::Low),
                capabilities: profile.capabilities.clone(),
                context_tokens: profile.context_tokens,
                ttl_seconds: 60,
                qualification: false,
            };
            let mut demand = Admission {
                service: &self.service,
                source,
            }
            .prepare(request, 0)
            .unwrap();
            demand.state = DemandState::Ready;
            demand.ready_checked = true;
            self.service
                .authority
                .update(|s| {
                    for resource in demand.profile.resources.clone() {
                        s.claims.insert(resource, demand.id.clone());
                    }
                    s.data.demands.insert(demand.id.clone(), demand.clone());
                    Ok(())
                })
                .unwrap();
            self.service.inspect(&demand.owner, &demand.id).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.service.config.authority_root);
        }
    }

    fn request(demand: &Demand, submission_id: &str) -> Inference {
        Inference {
            work: None,
            schema: VERSION.into(),
            submission_id: submission_id.into(),
            reservation_id: demand.id.clone(),
            generation: demand.generation.clone(),
            profile_hash: demand.profile_hash.clone(),
            prompt: "original prompt survives rollback".into(),
            payload: None,
            max_tokens: 16,
            timeout_seconds: 5,
            wait_seconds: None,
            workspace_scope: None,
        }
    }

    fn completion_value(demand: &Demand) -> serde_json::Value {
        json!({
            "id": "fixture-completion",
            "model": demand.profile.model,
            "choices": [{"message": {"role": "assistant", "content": "ok"}}],
            "usage": {"completion_tokens": 1}
        })
    }

    #[test]
    fn paused_history_publication_does_not_block_unrelated_execution_or_release() {
        let fixture = Fixture::new();
        let old = fixture.acquire_ready();
        let old_invocation = crate::inference::Submission {
            service: &fixture.service,
        }
        .submit(&old.owner, request(&old, "old-terminal"))
        .unwrap();
        let old_running = fixture
            .service
            .authority
            .update(|s| {
                let invocation = s.data.invocations.get_mut(&old_invocation.id).unwrap();
                invocation.state = InvocationState::Running;
                invocation.started = Some(now());
                invocation.execution_deadline = Some(now() + 5);
                invocation.activation = Some(old.generation.clone());
                Ok(invocation.clone())
            })
            .unwrap();
        Completion {
            service: &fixture.service,
        }
        .save((old_running, old.clone()), Ok(completion_value(&old)))
        .unwrap();

        let pause = crate::history_archive::MaintenanceIoPause::new();
        crate::history_archive::install_maintenance_publish_io_pause(std::sync::Arc::clone(&pause));
        let service = std::sync::Arc::clone(&fixture.service);
        let maintenance = std::thread::spawn(move || service.retire_history_once());
        pause.wait_until_entered();

        let current = fixture.acquire_ready();
        let invocation = crate::inference::Submission {
            service: &fixture.service,
        }
        .submit(&current.owner, request(&current, "unrelated-live"))
        .unwrap();
        let running = fixture
            .service
            .authority
            .update(|s| {
                let invocation = s.data.invocations.get_mut(&invocation.id).unwrap();
                invocation.state = InvocationState::Running;
                invocation.started = Some(now());
                invocation.execution_deadline = Some(now() + 5);
                invocation.activation = Some(current.generation.clone());
                Ok(invocation.clone())
            })
            .unwrap();
        Completion {
            service: &fixture.service,
        }
        .save((running, current.clone()), Ok(completion_value(&current)))
        .unwrap();
        let completed = fixture
            .service
            .result(&current.owner, &invocation.id)
            .unwrap();
        assert_eq!(completed.state, InvocationState::Completed);

        crate::control::ReservationControl {
            service: &fixture.service,
        }
        .control(crate::control::ControlContext {
            owner: &current.owner,
            id: &current.id,
            request: crate::control::Control {
                generation: current.generation.clone(),
                action: crate::control::Action::Release,
            },
        })
        .unwrap();
        let releasing = fixture
            .service
            .inspect(&current.owner, &current.id)
            .unwrap();
        crate::retirement::Retirement {
            service: &fixture.service,
        }
        .run(&releasing)
        .unwrap();
        let released = fixture
            .service
            .inspect(&current.owner, &current.id)
            .unwrap();
        assert_eq!(released.state, DemandState::Released);

        pause.release();
        let report = maintenance.join().unwrap().unwrap();
        crate::history_archive::clear_maintenance_publish_io_pause();
        assert!(report.archived_invocations >= 1);
    }

    #[test]
    fn completion_finalization_failure_preserves_committed_request_sidecar() {
        let fixture = Fixture::new();
        let demand = fixture.acquire_ready();
        let submitted = crate::inference::Submission {
            service: &fixture.service,
        }
        .submit(&demand.owner, request(&demand, "completion-rollback"))
        .unwrap();
        let running = fixture
            .service
            .authority
            .update(|s| {
                let invocation = s.data.invocations.get_mut(&submitted.id).unwrap();
                assert!(invocation.request_ref.is_some());
                invocation.state = InvocationState::Running;
                invocation.started = Some(now());
                invocation.execution_deadline = Some(now() + 5);
                invocation.activation = Some(demand.generation.clone());
                Ok(invocation.clone())
            })
            .unwrap();

        fail_completion_after_payload_for(&submitted.id);
        let failure = Completion {
            service: &fixture.service,
        }
        .save(
            (running.clone(), demand.clone()),
            Ok(completion_value(&demand)),
        )
        .unwrap_err();
        assert_eq!(failure, "injected_completion_finalization_failure");

        let committed = fixture
            .service
            .authority
            .read(|s| Ok(s.data.invocations.get(&submitted.id).unwrap().clone()))
            .unwrap();
        assert_eq!(committed.state, InvocationState::Running);
        assert!(committed.request_ref.is_some());
        assert!(committed.result_ref.is_none());
        let hydrated = crate::payload_store::hydrate_invocation(
            &fixture.service.config.authority_root,
            committed.clone(),
        )
        .unwrap();
        assert_eq!(hydrated.request.prompt, "original prompt survives rollback");

        Completion {
            service: &fixture.service,
        }
        .save((running, demand.clone()), Ok(completion_value(&demand)))
        .unwrap();
        let completed = fixture
            .service
            .result(&demand.owner, &submitted.id)
            .unwrap();
        assert_eq!(completed.state, InvocationState::Completed);
        assert_eq!(
            completed.request.prompt,
            "original prompt survives rollback"
        );
    }
}
