use crate::{
    service::{Service, owns},
    types::*,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const INTERVAL: Duration = Duration::from_secs(60);
const WORKSPACE_RECOVERY_BLOCKER: &str = "recovery_active_or_unproven_workspace_invocation";

pub struct Monitor {
    pub service: Arc<Service>,
}

impl Monitor {
    pub fn run_forever(self) {
        loop {
            if let Err(error) = self.tick() {
                eprintln!("workspace recovery monitor failed: {error}");
            }
            std::thread::sleep(INTERVAL);
        }
    }

    pub fn tick(&self) -> Result<(), String> {
        let candidates = self.service.authority.read(|s| Ok(candidates(s)))?;
        for candidate in candidates {
            let analysis = match analyze(&self.service, &candidate) {
                Ok(analysis) => analysis,
                Err(error) => {
                    eprintln!(
                        "workspace recovery diagnosis skipped: demand={} invocation={} error={}",
                        candidate.demand.id, candidate.invocation.id, error
                    );
                    continue;
                }
            };
            if persist(&self.service, &candidate, analysis.clone())? {
                eprintln!(
                    "workspace recovery diagnosis persisted: demand={} invocation={} status={} packet={}",
                    candidate.demand.id,
                    candidate.invocation.id,
                    analysis.packet_status,
                    analysis.packet_path
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Candidate {
    demand: Demand,
    invocation: Invocation,
}

fn candidates(s: &Document) -> Vec<Candidate> {
    s.data
        .demands
        .values()
        .filter(|d| {
            d.state == DemandState::RecoveryRequired
                && d.reason.as_deref() == Some("invocation_outcome_unknown")
                && owns(s, d)
        })
        .flat_map(|d| {
            s.data
                .invocations
                .values()
                .filter(move |i| {
                    i.request.reservation_id == d.id
                        && i.state == InvocationState::Uncertain
                        && crate::work_payload::is_workspace(i)
                        && !analysis_recorded(d, i)
                })
                .cloned()
                .map(|invocation| Candidate {
                    demand: d.clone(),
                    invocation,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn allows_recovery(s: &Document, d: &Demand, i: &Invocation) -> bool {
    i.state == InvocationState::Uncertain
        && crate::work_payload::is_workspace(i)
        && analysis_recorded(d, i)
        && scoped_children_physically_recoverable(s, d, i)
        && workspace_scope_closed_or_expired(s, &i.id)
}

fn analysis_recorded(d: &Demand, i: &Invocation) -> bool {
    d.workspace_recovery_analyses
        .get(&i.id)
        .is_some_and(|analysis| {
            analysis.invocation_id == i.id
                && terminal_packet_status(&analysis.packet_status)
                && analysis.checks.retained_terminal_packet
                && (analysis.checks.scoped_children_terminal
                    || analysis.checks.scoped_children_physically_recoverable)
                && analysis.checks.workspace_scope_closed_or_expired
                && analysis.checks.packet_under_state_root
                && analysis.checks.ownership_fence_intact
        })
}

fn analyze(service: &Service, candidate: &Candidate) -> Result<WorkspaceRecoveryAnalysis, String> {
    if candidate.invocation.error.as_deref() != Some("workspace_model_outcome_uncertain") {
        return Err("workspace_recovery_unsupported_parent_error".into());
    }
    let (scoped_children, scope_ok) = service.authority.read(|s| {
        fence(s, &candidate.demand)?;
        let parent = workspace_parent(s, candidate)?;
        Ok((
            scoped_child_summaries(s, &candidate.demand, parent),
            workspace_scope_closed_or_expired(s, &parent.id),
        ))
    })?;
    if !scope_ok {
        return Err("workspace_recovery_scope_open_or_missing".into());
    }
    let scoped_children_terminal = scoped_children_terminal_from_summaries(&scoped_children);
    let scoped_children_physically_recoverable =
        scoped_children_physically_recoverable_from_summaries(&scoped_children);
    if !scoped_children_physically_recoverable {
        return Err("workspace_recovery_scoped_child_unproven".into());
    }
    let result = candidate
        .invocation
        .late_result
        .as_ref()
        .or(candidate.invocation.result.as_ref())
        .ok_or("workspace_recovery_parent_result_missing")?;
    let packet_path = result
        .get("packet_path")
        .and_then(Value::as_str)
        .ok_or("workspace_recovery_packet_path_missing")?;
    let (packet_path, packet) = read_packet(service, packet_path)?;
    let packet_status = required_string(&packet, "status")?;
    if !terminal_packet_status(&packet_status) {
        return Err("workspace_recovery_packet_not_terminal".into());
    }
    let work = candidate.invocation.request.work.as_ref();
    let workspace = work.and_then(crate::work_payload::Work::workspace);
    Ok(WorkspaceRecoveryAnalysis {
        invocation_id: candidate.invocation.id.clone(),
        diagnosed_at: now(),
        parent_error: candidate.invocation.error.clone(),
        work_id: work.map(|work| work.work_id.clone()),
        repository_id: workspace.map(|workspace| workspace.repository.id.clone()),
        repository_root: workspace
            .and_then(|workspace| workspace.repository.root.as_ref().cloned()),
        packet_path: packet_path.display().to_string(),
        packet_status,
        packet_acceptance_verdict: optional_string(&packet, "acceptance_verdict"),
        packet_last_error: optional_string(&packet, "last_error"),
        scoped_children,
        checks: WorkspaceRecoveryChecks {
            retained_terminal_packet: true,
            scoped_children_terminal,
            scoped_children_physically_recoverable,
            workspace_scope_closed_or_expired: true,
            packet_under_state_root: true,
            ownership_fence_intact: true,
        },
    })
}

fn persist(
    service: &Service,
    candidate: &Candidate,
    analysis: WorkspaceRecoveryAnalysis,
) -> Result<bool, String> {
    service.authority.update(|s| {
        fence(s, &candidate.demand)?;
        let parent = workspace_parent(s, candidate)?;
        let parent_id = parent.id.clone();
        if !scoped_children_physically_recoverable(s, &candidate.demand, parent) {
            return Err("workspace_recovery_scoped_child_unproven".into());
        }
        if !workspace_scope_closed_or_expired(s, &parent_id) {
            return Err("workspace_recovery_scope_open_or_missing".into());
        }
        let saved = s
            .data
            .demands
            .get_mut(&candidate.demand.id)
            .ok_or("missing_transition")?;
        if saved.workspace_recovery_analyses.contains_key(&parent_id) {
            return Ok(false);
        }
        saved
            .workspace_recovery_analyses
            .insert(parent_id, analysis);
        saved.recovery_error = Some(WORKSPACE_RECOVERY_BLOCKER.into());
        saved.transition_deadline = now();
        crate::capacity::retention(s, &service.config.limits)?;
        Ok(true)
    })
}

fn fence(s: &Document, d: &Demand) -> Result<(), String> {
    let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
    if saved.generation != d.generation
        || saved.profile_hash != d.profile_hash
        || saved.process != d.process
        || saved.state != DemandState::RecoveryRequired
        || !owns(s, saved)
    {
        return Err("workspace_recovery_ownership_fence_changed".into());
    }
    Ok(())
}

fn workspace_parent<'a>(s: &'a Document, candidate: &Candidate) -> Result<&'a Invocation, String> {
    let parent = s
        .data
        .invocations
        .get(&candidate.invocation.id)
        .ok_or("missing_invocation")?;
    if parent.request.reservation_id != candidate.demand.id
        || parent.state != InvocationState::Uncertain
        || parent.error.as_deref() != Some("workspace_model_outcome_uncertain")
        || !crate::work_payload::is_workspace(parent)
    {
        return Err("workspace_recovery_parent_changed".into());
    }
    Ok(parent)
}

fn scoped_child_summaries(
    s: &Document,
    demand: &Demand,
    parent: &Invocation,
) -> Vec<WorkspaceRecoveryChild> {
    s.data
        .invocations
        .values()
        .filter(|child| scoped_to_parent(s, child, &parent.id))
        .map(|child| WorkspaceRecoveryChild {
            invocation_id: child.id.clone(),
            state: child.state,
            error: child.error.clone(),
            workspace_scope: child.request.workspace_scope.clone(),
            reservation_id: child.request.reservation_id.clone(),
            request_generation: child.request.generation.clone(),
            request_profile_hash: child.request.profile_hash.clone(),
            invocation_activation: child.activation.clone(),
            demand_backend_activation: demand.backend_activation.clone(),
            physical_effect: child_physical_effect(s, demand, parent, child),
            started: child.started,
            execution_deadline: child.execution_deadline,
            cancellation_requested_at: child.cancellation.as_ref().map(|value| value.requested_at),
            result_present: child.result.is_some(),
            late_result_present: child.late_result.is_some(),
        })
        .collect()
}

fn scoped_children_physically_recoverable(
    s: &Document,
    demand: &Demand,
    parent: &Invocation,
) -> bool {
    let children = scoped_child_summaries(s, demand, parent);
    scoped_children_physically_recoverable_from_summaries(&children)
}

fn scoped_children_terminal_from_summaries(children: &[WorkspaceRecoveryChild]) -> bool {
    children
        .iter()
        .all(|child| child.physical_effect == WorkspaceRecoveryChildPhysicalEffect::Terminal)
}

fn scoped_children_physically_recoverable_from_summaries(
    children: &[WorkspaceRecoveryChild],
) -> bool {
    children.iter().all(|child| {
        matches!(
            child.physical_effect,
            WorkspaceRecoveryChildPhysicalEffect::Terminal
                | WorkspaceRecoveryChildPhysicalEffect::ConfinedToDemandBackend
        )
    })
}

fn child_physical_effect(
    s: &Document,
    demand: &Demand,
    parent: &Invocation,
    child: &Invocation,
) -> WorkspaceRecoveryChildPhysicalEffect {
    match child.state {
        InvocationState::Queued | InvocationState::Running => {
            WorkspaceRecoveryChildPhysicalEffect::UnresolvedActive
        }
        InvocationState::Uncertain
            if uncertain_child_confined_to_demand_backend(s, demand, parent, child) =>
        {
            WorkspaceRecoveryChildPhysicalEffect::ConfinedToDemandBackend
        }
        InvocationState::Uncertain => WorkspaceRecoveryChildPhysicalEffect::IndependentUnproven,
        InvocationState::Completed
        | InvocationState::Cancelled
        | InvocationState::Failed
        | InvocationState::Expired => WorkspaceRecoveryChildPhysicalEffect::Terminal,
    }
}

fn uncertain_child_confined_to_demand_backend(
    s: &Document,
    demand: &Demand,
    parent: &Invocation,
    child: &Invocation,
) -> bool {
    child.request.reservation_id == demand.id
        && child.request.generation == demand.generation
        && child.request.profile_hash == demand.profile_hash
        && child.activation.as_deref() == Some(demand.generation.as_str())
        && child.request.work.is_none()
        && demand
            .backend_activation
            .as_deref()
            .is_some_and(|activation| !activation.is_empty())
        && child
            .request
            .workspace_scope
            .as_ref()
            .and_then(|scope| s.data.workspace_scopes.get(scope))
            .is_some_and(|scope| scope.invocation_id.as_deref() == Some(parent.id.as_str()))
}

fn scoped_to_parent(s: &Document, child: &Invocation, parent_id: &str) -> bool {
    child
        .request
        .workspace_scope
        .as_ref()
        .and_then(|scope| s.data.workspace_scopes.get(scope))
        .is_some_and(|scope| scope.invocation_id.as_deref() == Some(parent_id))
}

fn workspace_scope_closed_or_expired(s: &Document, parent_id: &str) -> bool {
    let mut found = false;
    let now_ms = crate::workspace_scope::now_ms();
    for scope in s
        .data
        .workspace_scopes
        .values()
        .filter(|scope| scope.invocation_id.as_deref() == Some(parent_id))
    {
        found = true;
        if scope.closed_at_ms.is_none() && scope.deadline_ms > now_ms {
            return false;
        }
    }
    found
}

fn read_packet(service: &Service, packet_path: &str) -> Result<(PathBuf, Value), String> {
    let config = service
        .config
        .workspace
        .as_ref()
        .ok_or("workspace_not_configured")?;
    let path = PathBuf::from(packet_path);
    if !path.is_absolute() {
        return Err("workspace_recovery_packet_path_relative".into());
    }
    let packet = path
        .canonicalize()
        .map_err(|error| format!("workspace_recovery_packet_unreadable:{error}"))?;
    let changes_root = config.state_root.join("state").join("changes");
    let changes_root = canonicalize_existing_directory(&changes_root)?;
    if !packet.starts_with(&changes_root) {
        return Err("workspace_recovery_packet_outside_state_root".into());
    }
    let raw = std::fs::read_to_string(&packet)
        .map_err(|error| format!("workspace_recovery_packet_unreadable:{error}"))?;
    let value = serde_json::from_str(&raw)
        .map_err(|error| format!("workspace_recovery_packet_invalid:{error}"))?;
    Ok((packet, value))
}

fn canonicalize_existing_directory(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("workspace_recovery_state_root_unreadable:{error}"))?;
    if !canonical.is_dir() {
        return Err("workspace_recovery_state_root_invalid".into());
    }
    Ok(canonical)
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("workspace_recovery_packet_{key}_missing"))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn terminal_packet_status(value: &str) -> bool {
    matches!(
        value,
        "checks_passed"
            | "checks_failed"
            | "executor_unavailable"
            | "failed"
            | "path_policy_failed"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Config, Driver},
        transition::Transition,
        work_payload::WorkspaceConfig,
    };
    use serde_json::json;
    use std::{collections::BTreeMap, fs, path::PathBuf};

    struct Fixture {
        service: Arc<Service>,
        state_root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let mut config: Config =
                serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                    .unwrap();
            let id = identity().unwrap();
            let root = std::env::temp_dir().join(format!("rack-workspace-recovery-{id}"));
            let authority_root = root.join("authority");
            let state_root = root.join("workspace");
            fs::create_dir_all(state_root.join("state/changes")).unwrap();
            fs::create_dir_all(&authority_root).unwrap();
            config.authority_root = authority_root;
            config.workspace = Some(WorkspaceConfig {
                registry_root: state_root.clone(),
                state_root: state_root.clone(),
            });
            config.fixture_mode = true;
            let fixture_executable = PathBuf::from("/bin/sleep");
            let fixture_executable_sha256 = digest(&fs::read(&fixture_executable).unwrap());
            for profile in &mut config.profiles {
                profile.driver = Driver::Fixture;
                profile.qualified = true;
                profile.evidence = vec!["synthetic-no-gpu".into()];
                profile.executable = fixture_executable.clone();
                profile.executable_sha256 = fixture_executable_sha256.clone();
                profile.args = vec!["60".into()];
                profile.stop_seconds = 2;
            }
            Self {
                service: Arc::new(Service::new(config)),
                state_root,
            }
        }

        fn demand(&self) -> Demand {
            let profile = self
                .service
                .config
                .profiles
                .iter()
                .find(|profile| profile.tag == "local-primary")
                .unwrap()
                .clone();
            Demand {
                reservation_id: None,
                services: BTreeMap::new(),
                reserve_request: None,
                reserve_result: None,
                reservation_closed: None,
                recovery_reconciliation: None,
                recovery_error: Some(WORKSPACE_RECOVERY_BLOCKER.into()),
                workspace_recovery_analyses: BTreeMap::new(),
                id: "demand".into(),
                owner: "athba".into(),
                request: Acquire {
                    schema: VERSION.into(),
                    source_system: "athba".into(),
                    work_id: "reservation-work".into(),
                    acquisition_id: "acquire".into(),
                    tag: "local-primary".into(),
                    priority: Some(Priority::Low),
                    capabilities: profile.capabilities.clone(),
                    context_tokens: profile.context_tokens,
                    ttl_seconds: 3600,
                    qualification: false,
                },
                priority: Priority::Low,
                profile,
                profile_hash: "profile-hash".into(),
                state: DemandState::RecoveryRequired,
                reason: Some("invocation_outcome_unknown".into()),
                retry_after: None,
                preempted_by: None,
                ready_checked: true,
                accepted_calls: 1,
                generation: "generation".into(),
                access_key: "access-key".into(),
                backend_activation: Some("activation".into()),
                created: now(),
                last_activity_at: None,
                order: 1,
                deadline: now() + 3600,
                transition_deadline: now(),
                victims: Vec::new(),
                process: None,
                effect_started: false,
                preflight_done: true,
                released: false,
            }
        }

        fn packet(&self, outside_root: bool) -> String {
            let path = if outside_root {
                self.service
                    .config
                    .authority_root
                    .join("outside-packet.json")
            } else {
                let dir = self.state_root.join("state/changes/work-fixture");
                fs::create_dir_all(&dir).unwrap();
                dir.join("review-packet.json")
            };
            fs::write(
                &path,
                serde_json::to_string(&json!({
                    "change_id": "work-fixture",
                    "repository_id": "repo",
                    "registered_root": "/tmp/repo",
                    "base_ref": "main",
                    "base_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "branch": "rack/change-work-fixture",
                    "worktree_path": "/tmp/worktree",
                    "task": "fixture",
                    "allowed_paths": ["tests/test_running_total.py"],
                    "changed_paths": ["tests/test_running_total.py"],
                    "git_status": " M tests/test_running_total.py",
                    "diff_stat": "tests/test_running_total.py | 1 +",
                    "diff": "",
                    "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "commands": [],
                    "required_artifacts": [],
                    "implementer_output": "fixture timeout",
                    "acceptance_verdict": "rejected",
                    "status": "failed",
                    "retention": "retained",
                    "last_error": "jcode wall-clock timeout exceeded for worker local-primary"
                }))
                .unwrap(),
            )
            .unwrap();
            path.display().to_string()
        }

        fn work() -> crate::work_payload::Work {
            serde_json::from_value(json!({
                "reservation_id": "logical-reservation",
                "service": "local-primary",
                "work_id": "workspace-work",
                "payload": {
                    "kind": "workspace",
                    "workspace": {
                        "repository": {
                            "id": "repo",
                            "root": "/srv/ATHBA/state/projects/fixture/repository",
                            "base_ref": "main",
                            "base_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        },
                        "objective": "Add one test.",
                        "allowed_paths": ["tests/test_running_total.py"],
                        "acceptance": {"commands": [["pytest", "tests/test_running_total.py"]]},
                        "requirements": {"complexity": "medium", "requires_large_context": false},
                        "limits": {"max_implementation_attempts": 1, "timeout_seconds": 300}
                    }
                }
            }))
            .unwrap()
        }

        fn invocation(&self, demand: &Demand, packet_path: &str) -> Invocation {
            Invocation {
                scope_access_hash: Some(digest(demand.access_key.as_bytes())),
                id: "parent-invocation".into(),
                owner: demand.owner.clone(),
                request: Inference {
                    work: Some(Self::work()),
                    schema: VERSION.into(),
                    submission_id: "work-submission".into(),
                    reservation_id: demand.id.clone(),
                    generation: demand.generation.clone(),
                    profile_hash: demand.profile_hash.clone(),
                    prompt: String::new(),
                    payload: None,
                    max_tokens: 1,
                    timeout_seconds: 300,
                    wait_seconds: None,
                    workspace_scope: None,
                },
                request_digest: None,
                request_bytes: None,
                work_digest: None,
                work_bytes: None,
                state: InvocationState::Uncertain,
                queue_order: 1,
                waiting_deadline: now() + 60,
                execution_deadline: Some(now() - 1),
                response_bytes: 64 * 1024,
                cancellation: None,
                late_result: Some(json!({
                    "work_id": "workspace-work",
                    "change_id": "work-fixture",
                    "status": "failed",
                    "acceptance_verdict": "rejected",
                    "packet_path": packet_path
                })),
                result_digest: None,
                late_result_digest: None,
                started: Some(now() - 300),
                activation: Some(demand.generation.clone()),
                result: None,
                error: Some("workspace_model_outcome_uncertain".into()),
            }
        }

        fn seed(&self, active_child: bool, outside_packet_root: bool) -> (Demand, Invocation) {
            let demand = self.demand();
            let packet_path = self.packet(outside_packet_root);
            let parent = self.invocation(&demand, &packet_path);
            self.service
                .authority
                .update(|s| {
                    let mut ready = demand.clone();
                    ready.state = DemandState::Ready;
                    ready.reason = None;
                    ready.recovery_error = None;
                    s.claims.insert("gpu-4060ti".into(), ready.id.clone());
                    s.data.demands.insert(ready.id.clone(), ready);
                    let mut running_parent = parent.clone();
                    running_parent.state = InvocationState::Running;
                    running_parent.error = None;
                    running_parent.late_result = None;
                    running_parent.execution_deadline = Some(now() + 300);
                    s.data
                        .invocations
                        .insert(running_parent.id.clone(), running_parent);
                    Ok(())
                })
                .unwrap();
            crate::workspace_scope::ScopeController {
                service: &self.service,
            }
            .control(
                crate::workspace_scope::ScopeAccess {
                    reservation: demand.id.clone(),
                    capability: demand.access_key.clone(),
                    namespace: "workspace".into(),
                },
                crate::workspace_scope::ScopeControl::Open {
                    deadline_ms: crate::workspace_scope::now_ms() + 60_000,
                    invocation_id: Some(parent.id.clone()),
                },
            )
            .unwrap();
            let scope_id = crate::workspace_scope::key(&demand.id, "workspace");
            if !active_child {
                crate::workspace_scope::ScopeController {
                    service: &self.service,
                }
                .control(
                    crate::workspace_scope::ScopeAccess {
                        reservation: demand.id.clone(),
                        capability: demand.access_key.clone(),
                        namespace: "workspace".into(),
                    },
                    crate::workspace_scope::ScopeControl::Close,
                )
                .unwrap();
            }
            self.service
                .authority
                .update(|s| {
                    s.data.demands.insert(demand.id.clone(), demand.clone());
                    s.claims.insert("gpu-4060ti".into(), demand.id.clone());
                    s.data.invocations.insert(parent.id.clone(), parent.clone());
                    s.data.invocations.insert(
                        "child-invocation".into(),
                        Invocation {
                            scope_access_hash: None,
                            id: "child-invocation".into(),
                            owner: demand.owner.clone(),
                            request: Inference {
                                work: None,
                                schema: VERSION.into(),
                                submission_id: "child-submission".into(),
                                reservation_id: demand.id.clone(),
                                generation: demand.generation.clone(),
                                profile_hash: demand.profile_hash.clone(),
                                prompt: String::new(),
                                payload: None,
                                max_tokens: 16,
                                timeout_seconds: 60,
                                wait_seconds: None,
                                workspace_scope: Some(scope_id),
                            },
                            request_digest: None,
                            request_bytes: None,
                            work_digest: None,
                            work_bytes: None,
                            state: if active_child {
                                InvocationState::Running
                            } else {
                                InvocationState::Cancelled
                            },
                            queue_order: 2,
                            waiting_deadline: now() + 60,
                            execution_deadline: Some(now() + 60),
                            response_bytes: 1024,
                            cancellation: if active_child {
                                None
                            } else {
                                Some(CancellationIntent {
                                    requested_at: now(),
                                })
                            },
                            late_result: if active_child {
                                None
                            } else {
                                Some(json!({"late": true}))
                            },
                            result_digest: None,
                            late_result_digest: None,
                            started: Some(now() - 1),
                            activation: Some(demand.generation.clone()),
                            result: None,
                            error: None,
                        },
                    );
                    Ok(())
                })
                .unwrap();
            (demand, parent)
        }

        fn live_deadlock_pattern(&self, demand: Demand, owned_effect: bool) -> Demand {
            let mut demand = demand;
            if owned_effect {
                let process = crate::hosting::Hosting {
                    config: &self.service.config,
                }
                .start(&demand)
                .unwrap();
                demand.process = Some(process);
                demand.effect_started = true;
            }
            let scope_id = crate::workspace_scope::key(&demand.id, "workspace");
            let children = vec![
                self.child(
                    &demand,
                    &scope_id,
                    "terminal-completed",
                    InvocationState::Completed,
                    None,
                    false,
                    false,
                    None,
                ),
                self.child(
                    &demand,
                    &scope_id,
                    "terminal-cancelled",
                    InvocationState::Cancelled,
                    None,
                    false,
                    false,
                    None,
                ),
                self.child(
                    &demand,
                    &scope_id,
                    "terminal-failed",
                    InvocationState::Failed,
                    Some("fixture_failed"),
                    false,
                    false,
                    None,
                ),
                self.child(
                    &demand,
                    &scope_id,
                    "terminal-expired",
                    InvocationState::Expired,
                    Some("fixture_expired"),
                    false,
                    false,
                    None,
                ),
                self.child(
                    &demand,
                    &scope_id,
                    "uncertain-oversized",
                    InvocationState::Uncertain,
                    Some("backend_response_oversized"),
                    false,
                    false,
                    None,
                ),
            ];
            self.replace_scoped_children(&demand, &scope_id, children);
            demand
        }

        #[allow(clippy::too_many_arguments)]
        fn child(
            &self,
            demand: &Demand,
            scope_id: &str,
            id: &str,
            state: InvocationState,
            error: Option<&str>,
            nested_work: bool,
            generation_mismatch: bool,
            activation: Option<String>,
        ) -> Invocation {
            let terminal = !matches!(
                state,
                InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
            );
            let generation = if generation_mismatch {
                "other-generation".to_string()
            } else {
                demand.generation.clone()
            };
            Invocation {
                scope_access_hash: None,
                id: id.into(),
                owner: demand.owner.clone(),
                request: Inference {
                    work: nested_work.then(Self::work),
                    schema: VERSION.into(),
                    submission_id: format!("{id}-submission"),
                    reservation_id: demand.id.clone(),
                    generation,
                    profile_hash: demand.profile_hash.clone(),
                    prompt: String::new(),
                    payload: None,
                    max_tokens: 16,
                    timeout_seconds: 60,
                    wait_seconds: None,
                    workspace_scope: Some(scope_id.to_string()),
                },
                request_digest: None,
                request_bytes: None,
                work_digest: None,
                work_bytes: None,
                state,
                queue_order: 2,
                waiting_deadline: now() + 60,
                execution_deadline: if state == InvocationState::Queued {
                    None
                } else if state == InvocationState::Running {
                    Some(now() + 60)
                } else {
                    Some(now() - 1)
                },
                response_bytes: 1024,
                cancellation: matches!(
                    state,
                    InvocationState::Cancelled | InvocationState::Uncertain
                )
                .then(|| CancellationIntent {
                    requested_at: now(),
                }),
                late_result: terminal.then(|| json!({"late": true})),
                result_digest: None,
                late_result_digest: None,
                started: (state != InvocationState::Queued).then(|| now() - 1),
                activation: activation.or_else(|| {
                    (state != InvocationState::Queued).then(|| demand.generation.clone())
                }),
                result: None,
                error: error.map(str::to_string),
            }
        }

        fn replace_scoped_children(
            &self,
            demand: &Demand,
            scope_id: &str,
            children: Vec<Invocation>,
        ) {
            self.service
                .authority
                .update(|s| {
                    s.data.demands.insert(demand.id.clone(), demand.clone());
                    s.claims.insert("gpu-4060ti".into(), demand.id.clone());
                    s.data.invocations.retain(|_, invocation| {
                        invocation.request.workspace_scope.as_deref() != Some(scope_id)
                    });
                    for child in children {
                        s.data.invocations.insert(child.id.clone(), child);
                    }
                    Ok(())
                })
                .unwrap();
        }

        fn mutate_scoped_child(&self, id: &str, mutate: impl FnOnce(&mut Invocation)) {
            self.service
                .authority
                .update(|s| {
                    let child = s.data.invocations.get_mut(id).unwrap();
                    mutate(child);
                    Ok(())
                })
                .unwrap();
        }

        fn reopen_scope(&self, demand: &Demand) {
            let scope_id = crate::workspace_scope::key(&demand.id, "workspace");
            self.service
                .authority
                .update(|s| {
                    let scope = s.data.workspace_scopes.get_mut(&scope_id).unwrap();
                    scope.closed_at_ms = None;
                    scope.deadline_ms = crate::workspace_scope::now_ms() + 60_000;
                    Ok(())
                })
                .unwrap();
        }

        fn parent_packet_path(parent: &Invocation) -> PathBuf {
            PathBuf::from(
                parent
                    .late_result
                    .as_ref()
                    .and_then(|value| value.get("packet_path"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap(),
            )
        }

        fn rewrite_packet_status(&self, parent: &Invocation, status: &str) {
            let path = Self::parent_packet_path(parent);
            let mut packet: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            packet["status"] = json!(status);
            fs::write(path, serde_json::to_string(&packet).unwrap()).unwrap();
        }

        fn assert_fenced(&self, demand: &Demand, parent: &Invocation) {
            Monitor {
                service: Arc::clone(&self.service),
            }
            .tick()
            .unwrap();
            self.service
                .authority
                .read(|s| {
                    let saved = s.data.demands.get(&demand.id).unwrap();
                    let invocation = s.data.invocations.get(&parent.id).unwrap();
                    assert!(!allows_recovery(s, saved, invocation));
                    assert!(!saved.workspace_recovery_analyses.contains_key(&parent.id));
                    Ok(())
                })
                .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let root = self
                .service
                .config
                .authority_root
                .parent()
                .unwrap()
                .to_path_buf();
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn monitor_interval_is_one_minute() {
        assert_eq!(INTERVAL, Duration::from_secs(60));
    }

    #[test]
    fn diagnosis_allows_existing_recovery_to_release_claim() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        fixture
            .service
            .authority
            .read(|s| {
                let saved = s.data.demands.get(&demand.id).unwrap();
                let invocation = s.data.invocations.get(&parent.id).unwrap();
                assert!(!allows_recovery(s, saved, invocation));
                Ok(())
            })
            .unwrap();

        Monitor {
            service: Arc::clone(&fixture.service),
        }
        .tick()
        .unwrap();

        fixture
            .service
            .authority
            .read(|s| {
                let saved = s.data.demands.get(&demand.id).unwrap();
                let invocation = s.data.invocations.get(&parent.id).unwrap();
                assert!(allows_recovery(s, saved, invocation));
                assert!(saved.workspace_recovery_analyses.contains_key(&parent.id));
                Ok(())
            })
            .unwrap();

        let fresh = fixture.service.inspect(&demand.owner, &demand.id).unwrap();
        Transition {
            service: &fixture.service,
        }
        .advance(&fresh)
        .unwrap();
        let released = fixture.service.inspect(&demand.owner, &demand.id).unwrap();
        assert_eq!(released.state, DemandState::Released);
        assert!(released.recovery_reconciliation.is_some());
        fixture
            .service
            .authority
            .read(|s| {
                assert!(s.claims.is_empty());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn confined_uncertain_scoped_child_allows_stop_only_recovery_without_rewriting_history() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        let demand = fixture.live_deadlock_pattern(demand, true);

        Monitor {
            service: Arc::clone(&fixture.service),
        }
        .tick()
        .unwrap();

        fixture
            .service
            .authority
            .read(|s| {
                let saved = s.data.demands.get(&demand.id).unwrap();
                let invocation = s.data.invocations.get(&parent.id).unwrap();
                let child = s.data.invocations.get("uncertain-oversized").unwrap();
                assert_eq!(child.state, InvocationState::Uncertain);
                assert_eq!(child.error.as_deref(), Some("backend_response_oversized"));
                assert!(allows_recovery(s, saved, invocation));
                let analysis = saved.workspace_recovery_analyses.get(&parent.id).unwrap();
                assert!(!analysis.checks.scoped_children_terminal);
                assert!(analysis.checks.scoped_children_physically_recoverable);
                let child_evidence = analysis
                    .scoped_children
                    .iter()
                    .find(|child| child.invocation_id == "uncertain-oversized")
                    .unwrap();
                assert_eq!(child_evidence.state, InvocationState::Uncertain);
                assert_eq!(
                    child_evidence.physical_effect,
                    WorkspaceRecoveryChildPhysicalEffect::ConfinedToDemandBackend
                );
                assert_eq!(child_evidence.request_generation, demand.generation);
                assert_eq!(
                    child_evidence.invocation_activation.as_deref(),
                    Some(demand.generation.as_str())
                );
                assert_eq!(
                    child_evidence.demand_backend_activation.as_deref(),
                    demand.backend_activation.as_deref()
                );
                Ok(())
            })
            .unwrap();

        let fresh = fixture.service.inspect(&demand.owner, &demand.id).unwrap();
        Transition {
            service: &fixture.service,
        }
        .advance(&fresh)
        .unwrap();
        let released = fixture.service.inspect(&demand.owner, &demand.id).unwrap();
        assert_eq!(released.state, DemandState::Released);
        let reconciliation = released.recovery_reconciliation.as_ref().unwrap();
        assert!(reconciliation.cleanup_process.is_some());
        assert_eq!(reconciliation.current_effect, CurrentEffect::ProvenAbsent);
        fixture
            .service
            .authority
            .read(|s| {
                assert!(s.claims.is_empty());
                let child = s.data.invocations.get("uncertain-oversized").unwrap();
                assert_eq!(child.state, InvocationState::Uncertain);
                assert_eq!(child.error.as_deref(), Some("backend_response_oversized"));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn queued_scoped_child_remains_fenced() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        let demand = fixture.live_deadlock_pattern(demand, false);
        fixture.mutate_scoped_child("uncertain-oversized", |child| {
            child.state = InvocationState::Queued;
            child.error = None;
            child.activation = None;
            child.started = None;
        });
        fixture.assert_fenced(&demand, &parent);
    }

    #[test]
    fn running_scoped_child_remains_fenced_after_scope_close() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        let demand = fixture.live_deadlock_pattern(demand, false);
        fixture.mutate_scoped_child("uncertain-oversized", |child| {
            child.state = InvocationState::Running;
            child.error = None;
            child.execution_deadline = Some(now() + 60);
        });
        fixture.assert_fenced(&demand, &parent);
    }

    #[test]
    fn uncertain_child_with_independent_work_remains_fenced() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        let demand = fixture.live_deadlock_pattern(demand, false);
        fixture.mutate_scoped_child("uncertain-oversized", |child| {
            child.request.work = Some(Fixture::work());
        });
        fixture.assert_fenced(&demand, &parent);
    }

    #[test]
    fn open_workspace_scope_remains_fenced() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        let demand = fixture.live_deadlock_pattern(demand, false);
        fixture.reopen_scope(&demand);
        fixture.assert_fenced(&demand, &parent);
    }

    #[test]
    fn missing_parent_packet_remains_fenced() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        let demand = fixture.live_deadlock_pattern(demand, false);
        fs::remove_file(Fixture::parent_packet_path(&parent)).unwrap();
        fixture.assert_fenced(&demand, &parent);
    }

    #[test]
    fn nonterminal_parent_packet_remains_fenced() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        let demand = fixture.live_deadlock_pattern(demand, false);
        fixture.rewrite_packet_status(&parent, "running");
        fixture.assert_fenced(&demand, &parent);
    }

    #[test]
    fn uncertain_child_generation_mismatch_remains_fenced() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, false);
        let demand = fixture.live_deadlock_pattern(demand, false);
        fixture.mutate_scoped_child("uncertain-oversized", |child| {
            child.request.generation = "other-generation".into();
        });
        fixture.assert_fenced(&demand, &parent);
    }

    #[test]
    fn active_scoped_child_remains_fenced() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(true, false);
        Monitor {
            service: Arc::clone(&fixture.service),
        }
        .tick()
        .unwrap();
        fixture
            .service
            .authority
            .read(|s| {
                let saved = s.data.demands.get(&demand.id).unwrap();
                let invocation = s.data.invocations.get(&parent.id).unwrap();
                assert!(!allows_recovery(s, saved, invocation));
                assert!(!saved.workspace_recovery_analyses.contains_key(&parent.id));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn packet_outside_state_root_remains_fenced() {
        let fixture = Fixture::new();
        let (demand, parent) = fixture.seed(false, true);
        Monitor {
            service: Arc::clone(&fixture.service),
        }
        .tick()
        .unwrap();
        fixture
            .service
            .authority
            .read(|s| {
                let saved = s.data.demands.get(&demand.id).unwrap();
                let invocation = s.data.invocations.get(&parent.id).unwrap();
                assert!(!allows_recovery(s, saved, invocation));
                assert!(!saved.workspace_recovery_analyses.contains_key(&parent.id));
                Ok(())
            })
            .unwrap();
    }
}
