use crate::{service::Service, types::*};
use rack_ai_application::ReviewPacket;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

const EXECUTION_SCHEMA: &str = "rack-ai/work-execution/v1";
const ARTIFACT_SCHEMA: &str = "rack-ai/work-artifact/v1";
pub const CONTRACT_VERSION: &str = "1.4.0";
const ARTIFACT_PREFIX: &str = "wa1";
const MAX_ARTIFACT_TEXT_BYTES: usize = 64 * 1024;
const MAX_DIAGNOSTIC_TEXT_BYTES: usize = 8 * 1024;

struct Snapshot {
    invocation: Invocation,
    demand: Option<Demand>,
    archived: bool,
    scoped_children: Option<ScopedCounts>,
}

#[derive(Default)]
struct ScopedCounts {
    total: usize,
    unresolved: usize,
    children: Vec<ScopedChild>,
}

struct ScopedChild {
    invocation_id: String,
    state: InvocationState,
    started: Option<u64>,
    completed: Option<u64>,
    error: Option<String>,
    usage: Option<Value>,
    finish_reason: Option<String>,
}

struct ArtifactKey {
    invocation_id: String,
    kind: String,
    index: usize,
}

struct PacketView {
    status: String,
    acceptance_verdict: Option<String>,
    accepted_revision: Option<String>,
    changed_paths: Vec<String>,
    commands: Vec<Value>,
    artifacts: Vec<Value>,
    evidence_status: String,
    last_error: Option<String>,
    any_command_timed_out: bool,
    activity_events: Vec<Value>,
    tool_calls: Vec<Value>,
    execution_budget_seconds: Option<u64>,
    deadline_ended_attempt: Option<String>,
}

pub fn inspect(service: &Service, input: (&str, &str)) -> Result<Value, String> {
    let owner = input.0;
    let snapshot = load_by_work(service, input)?;
    let packet = workspace_packet_view(service, owner, &snapshot.invocation)?;
    Ok(report(owner, &snapshot, packet.as_ref()))
}

pub fn artifact(service: &Service, input: (&str, &str)) -> Result<Value, String> {
    let owner = input.0;
    let key = parse_artifact_id(input.1)?;
    if artifact_id(owner, &key.invocation_id, &key.kind, key.index) != input.1 {
        return Err("not_found".into());
    }
    let invocation = service.result(owner, &key.invocation_id)?;
    if invocation.request.work.is_none() {
        return Err("not_found".into());
    }
    let packet = read_review_packet(service, &invocation)?.ok_or("not_found")?;
    let command = packet
        .commands()
        .get(key.index)
        .ok_or_else(|| "not_found".to_string())?;
    let text = match key.kind.as_str() {
        "stdout" => command.stdout(),
        "stderr" => command.stderr(),
        _ => return Err("not_found".into()),
    };
    if text.is_empty() {
        return Err("not_found".into());
    }
    let redacted = redact_diagnostic(text);
    let byte_len = redacted.len();
    let truncated = byte_len > MAX_ARTIFACT_TEXT_BYTES;
    let safe_text = if truncated {
        let mut end = MAX_ARTIFACT_TEXT_BYTES;
        while end > 0 && !redacted.is_char_boundary(end) {
            end -= 1;
        }
        redacted[..end].to_string()
    } else {
        redacted
    };
    Ok(json!({
        "schema": ARTIFACT_SCHEMA,
        "artifact_id": input.1,
        "invocation_id": key.invocation_id,
        "kind": key.kind,
        "index": key.index,
        "content_type": "text/plain; charset=utf-8",
        "text": safe_text,
        "bytes": byte_len,
        "truncated": truncated
    }))
}

fn load_by_work(service: &Service, input: (&str, &str)) -> Result<Snapshot, String> {
    let root = service.config.authority_root.clone();
    match service.authority.read(|s| {
        let invocation = crate::work::find(s, input.0, input.1)
            .cloned()
            .ok_or("not_found")?;
        let demand = s
            .data
            .demands
            .get(&invocation.request.reservation_id)
            .cloned();
        let scoped_children = scoped_counts(s, &invocation.id);
        Ok((invocation, demand, scoped_children))
    }) {
        Ok((invocation, demand, scoped_children)) => Ok(Snapshot {
            invocation: crate::payload_store::hydrate_invocation(&root, invocation)?,
            demand,
            archived: false,
            scoped_children: Some(scoped_children),
        }),
        Err(error) if error == "not_found" => {
            let invocation =
                crate::history_archive::lookup_work(&root, input.0, input.1)?.ok_or(error)?;
            Ok(Snapshot {
                invocation,
                demand: None,
                archived: true,
                scoped_children: None,
            })
        }
        Err(error) => Err(error),
    }
}

fn scoped_counts(s: &Document, parent_id: &str) -> ScopedCounts {
    let mut counts = ScopedCounts::default();
    for child in s.data.invocations.values().filter(|child| {
        child
            .request
            .workspace_scope
            .as_ref()
            .and_then(|scope| s.data.workspace_scopes.get(scope))
            .is_some_and(|scope| scope.invocation_id.as_deref() == Some(parent_id))
    }) {
        counts.total += 1;
        if matches!(
            child.state,
            InvocationState::Queued | InvocationState::Running
        ) {
            counts.unresolved += 1;
        }
        let result = child.result.as_ref().or(child.late_result.as_ref());
        counts.children.push(ScopedChild {
            invocation_id: child.id.clone(),
            state: child.state,
            started: child.started,
            completed: child.completed,
            error: child.error.clone(),
            usage: result.and_then(|value| value.get("usage")).cloned(),
            finish_reason: result
                .and_then(|value| value.get("choices"))
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("finish_reason"))
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    counts
}

fn report(owner: &str, snapshot: &Snapshot, packet: Option<&PacketView>) -> Value {
    let invocation = &snapshot.invocation;
    json!({
        "schema": EXECUTION_SCHEMA,
        "contract_version": CONTRACT_VERSION,
        "work": work_identity(snapshot),
        "outcome": outcome(invocation, packet),
        "closure": closure(snapshot),
        "activity": activity(snapshot, packet),
        "artifacts": packet.map(|p| p.artifacts.clone()).unwrap_or_default(),
        "build": {
            "runtime_schema": VERSION,
            "contract_version": CONTRACT_VERSION,
            "owner": owner
        }
    })
}

fn work_identity(snapshot: &Snapshot) -> Value {
    let invocation = &snapshot.invocation;
    let work = invocation.request.work.as_ref().expect("work identity");
    json!({
        "work_id": work.work_id,
        "reservation_id": work.reservation_id,
        "service": work.service,
        "invocation_id": invocation.id,
        "state": to_value(invocation.state),
        "archived": snapshot.archived,
        "created": optional_time(invocation.created),
        "started": invocation.started,
        "completed": invocation.completed
    })
}

fn outcome(invocation: &Invocation, packet: Option<&PacketView>) -> Value {
    let terminal = !matches!(
        invocation.state,
        InvocationState::Queued | InvocationState::Running
    );
    let workspace = packet.map(|packet| {
        json!({
            "status": packet.status,
            "acceptance_verdict": packet.acceptance_verdict,
            "accepted_revision": packet.accepted_revision,
            "changed_paths": packet.changed_paths,
            "commands": packet.commands.iter().map(redact_value).collect::<Vec<_>>(),
            "last_error": packet.last_error.as_deref().map(redact_diagnostic),
            "evidence_status": packet.evidence_status
        })
    });
    let historical_outcome_known = invocation.state != InvocationState::Uncertain;
    json!({
        "kind": if invocation.request.work.as_ref().is_some_and(|w| w.workspace().is_some()) {"workspace"} else {"inference"},
        "terminal": terminal,
        "outcome_known": historical_outcome_known,
        "category": outcome_category(invocation, packet),
        "failure_category": failure_category(invocation, packet),
        "error": invocation.error.as_deref().map(redact_diagnostic),
        "attempt": attempt_summary(invocation, packet),
        "historical_invocation": {
            "state": to_value(invocation.state),
            "terminal": terminal,
            "outcome_known": historical_outcome_known,
            "uncertainty_reason": if invocation.state == InvocationState::Uncertain {
                invocation.error.as_deref().map(redact_diagnostic)
            } else {
                None
            }
        },
        "workspace": workspace,
        "model_usage": model_usage(invocation)
    })
}

fn attempt_summary(invocation: &Invocation, packet: Option<&PacketView>) -> Value {
    let known = attempt_known(invocation, packet);
    json!({
        "known": known,
        "status": packet.map(|packet| packet.status.clone()).unwrap_or_else(|| to_string_value(invocation.state)),
        "category": if known { attempt_category(invocation, packet) } else { "unknown" },
        "failure_category": if known { failure_category(invocation, packet) } else { Some("outcome_uncertain") },
        "execution_budget_seconds": packet.and_then(|packet| packet.execution_budget_seconds),
        "deadline_ended_attempt": packet.and_then(|packet| packet.deadline_ended_attempt.clone())
    })
}

fn attempt_known(invocation: &Invocation, packet: Option<&PacketView>) -> bool {
    match packet {
        Some(packet) => packet.evidence_status == "recorded",
        None => !matches!(
            invocation.state,
            InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
        ),
    }
}

fn attempt_category(invocation: &Invocation, packet: Option<&PacketView>) -> &'static str {
    if let Some(packet) = packet {
        return workspace_category(Some(packet));
    }
    outcome_category(invocation, None)
}

fn closure(snapshot: &Snapshot) -> Value {
    let invocation = &snapshot.invocation;
    let demand = snapshot.demand.as_ref();
    let execution_active = matches!(
        invocation.state,
        InvocationState::Queued | InvocationState::Running
    );
    let cleanup_state = demand.map(cleanup_state).unwrap_or(if snapshot.archived {
        "archived"
    } else {
        "unavailable"
    });
    let safe_closure_known = matches!(cleanup_state, "safely_closed" | "not_required" | "archived");
    let replay_safety = if execution_active {
        "blocked_active_execution"
    } else if safe_closure_known {
        "closed_inspection_only"
    } else {
        "blocked_unresolved_effect"
    };
    json!({
        "execution_active": execution_active,
        "cleanup_state": cleanup_state,
        "original_outcome_known": invocation.state != InvocationState::Uncertain,
        "safe_closure_known": safe_closure_known,
        "replay_safety": replay_safety,
        "blocker": demand
            .and_then(|d| d.recovery_error.clone())
            .or_else(|| invocation.error.clone())
            .as_deref()
            .map(redact_diagnostic),
        "retry_after_seconds": demand.and_then(|d| d.retry_after)
    })
}

fn cleanup_state(demand: &Demand) -> &'static str {
    match demand.state {
        DemandState::RecoveryRequired => "recovery_required",
        DemandState::Releasing => "cleanup_pending",
        DemandState::Released
        | DemandState::Cancelled
        | DemandState::Expired
        | DemandState::Preempted => "safely_closed",
        _ => "not_required",
    }
}

fn activity(snapshot: &Snapshot, packet: Option<&PacketView>) -> Value {
    let invocation = &snapshot.invocation;
    json!({
        "activity_id": invocation.id,
        "sequence": invocation.queue_order,
        "current": current_activity(invocation),
        "last_observed_at": invocation.completed.or(invocation.started).or(optional_time(invocation.created)),
        "terminal_reason": invocation.error.as_deref().map(redact_diagnostic),
        "counts": {
            "model_calls": metric_count(workspace_model_calls(snapshot), workspace_children_availability(snapshot)),
            "scoped_children": metric_count(snapshot.scoped_children.as_ref().map(|c| c.total as u64), workspace_children_availability(snapshot)),
            "unresolved_scoped_children": metric_count(snapshot.scoped_children.as_ref().map(|c| c.unresolved as u64), workspace_children_availability(snapshot)),
            "command_errors": metric_count(packet.map(command_errors), packet_availability(packet)),
            "tool_calls": metric_count(packet.map(|packet| packet.tool_calls.len() as u64), packet_availability(packet))
        },
        "timings": {
            "queue_wait_seconds": metric_duration(queue_wait(invocation)),
            "active_execution_seconds": metric_duration(active_execution(invocation)),
            "setup_seconds": metric_duration(phase_duration(packet, "setup")),
            "agent_execution_seconds": metric_duration(phase_duration(packet, "agent_execution")),
            "acceptance_seconds": metric_duration(phase_duration(packet, "acceptance")),
            "cleanup_seconds": metric_duration(None),
            "first_token_seconds": metric_duration(None),
            "prefill_seconds": metric_duration(None),
            "backend_queue_seconds": metric_duration(None)
        },
        "budget": {
            "execution_seconds": metric_count(packet.and_then(|packet| packet.execution_budget_seconds), packet_availability(packet)),
            "deadline_ended_attempt": packet.and_then(|packet| packet.deadline_ended_attempt.clone())
        },
        "events": events(snapshot, packet)
    })
}

fn events(snapshot: &Snapshot, packet: Option<&PacketView>) -> Vec<Value> {
    let invocation = &snapshot.invocation;
    let mut events = Vec::new();
    if let Some(at) = optional_time(invocation.created) {
        events.push(json!({"kind":"queued","at":at}));
    }
    if let Some(at) = invocation.started {
        events.push(json!({"kind":"running","at":at}));
    }
    if let Some(packet) = packet {
        events.extend(packet.activity_events.iter().map(redact_value));
        for command in &packet.commands {
            let started = command.get("started").and_then(Value::as_u64);
            let completed = command.get("completed").and_then(Value::as_u64);
            events.push(json!({
                "kind": "command",
                "phase": "acceptance_command",
                "index": command.get("index").cloned().unwrap_or(Value::Null),
                "outcome": command.get("outcome").cloned().unwrap_or(Value::Null),
                "started": started,
                "completed": completed,
                "duration_seconds": started.zip(completed).map(|(started, completed)| completed.saturating_sub(started)),
                "timed_out": command.get("timed_out").cloned().unwrap_or(Value::Bool(false))
            }));
        }
        for (index, tool_call) in packet.tool_calls.iter().enumerate() {
            events.push(json!({
                "kind": "tool_call",
                "index": index,
                "name": tool_call.get("name").map(redact_value).unwrap_or(Value::Null),
                "outcome": tool_call.get("outcome").cloned().unwrap_or_else(|| json!("recorded")),
                "timing": metric_duration(None)
            }));
        }
    }
    if let Some(scoped_children) = snapshot.scoped_children.as_ref() {
        for child in &scoped_children.children {
            let duration = child
                .started
                .zip(child.completed)
                .map(|(started, completed)| completed.saturating_sub(started));
            events.push(json!({
                "kind": "model_call",
                "invocation_id": child.invocation_id,
                "state": to_value(child.state),
                "started": child.started,
                "completed": child.completed,
                "duration_seconds": duration,
                "finish_reason": child.finish_reason,
                "usage": child_usage(child),
                "error": child.error.as_deref().map(redact_diagnostic)
            }));
        }
    }
    if let Some(at) = invocation.completed {
        events.push(json!({"kind":"terminal","at":at,"state":to_value(invocation.state)}));
    }
    events
}

fn phase_duration(packet: Option<&PacketView>, phase: &str) -> Option<u64> {
    packet
        .into_iter()
        .flat_map(|packet| packet.activity_events.iter())
        .find(|event| event.get("phase").and_then(Value::as_str) == Some(phase))
        .and_then(|event| event.get("duration_seconds"))
        .and_then(Value::as_u64)
}

fn child_usage(child: &ScopedChild) -> Value {
    let usage = child.usage.as_ref();
    json!({
        "available": usage.is_some(),
        "prompt_tokens": usage.and_then(|u| u.get("prompt_tokens")).and_then(Value::as_u64),
        "completion_tokens": usage.and_then(|u| u.get("completion_tokens")).and_then(Value::as_u64),
        "total_tokens": usage.and_then(|u| u.get("total_tokens")).and_then(Value::as_u64)
    })
}

fn workspace_packet_view(
    service: &Service,
    owner: &str,
    invocation: &Invocation,
) -> Result<Option<PacketView>, String> {
    if !invocation
        .request
        .work
        .as_ref()
        .is_some_and(|work| work.workspace().is_some())
    {
        return Ok(None);
    }
    let packet = match read_review_packet(service, invocation) {
        Ok(Some(packet)) => packet,
        Ok(None) => return Ok(Some(unavailable_packet_view("packet_unavailable"))),
        Err(error)
            if matches!(
                error.as_str(),
                "packet_unavailable" | "packet_decode_failure"
            ) =>
        {
            return Ok(Some(unavailable_packet_view(&error)));
        }
        Err(error) => return Err(error),
    };
    let status = to_string_value(packet.status());
    let acceptance_verdict = packet.acceptance_verdict().map(to_string_value);
    let accepted_revision = if acceptance_verdict.as_deref() == Some("approved") {
        Some(packet.head_sha().to_string())
    } else {
        None
    };
    let mut artifacts = Vec::new();
    let commands = packet
        .commands()
        .iter()
        .enumerate()
        .map(|(index, command)| {
            let stdout_text = redact_diagnostic(command.stdout());
            let stderr_text = redact_diagnostic(command.stderr());
            let stdout = output_reference(owner, &invocation.id, "stdout", index, &stdout_text);
            let stderr = output_reference(owner, &invocation.id, "stderr", index, &stderr_text);
            if let Some(artifact) = stdout.get("artifact_id").and_then(Value::as_str) {
                artifacts.push(artifact_summary(artifact, "command_stdout", &stdout_text));
            }
            if let Some(artifact) = stderr.get("artifact_id").and_then(Value::as_str) {
                artifacts.push(artifact_summary(artifact, "command_stderr", &stderr_text));
            }
            json!({
                "index": index,
                "argv": command.argv().iter().map(|arg| redact_diagnostic(arg)).collect::<Vec<_>>(),
                "exit_code": command.exit_code(),
                "timed_out": command.timed_out(),
                "succeeded": command.succeeded(),
                "outcome": command_outcome(command.succeeded(), command.timed_out()),
                "started": command.started(),
                "completed": command.completed(),
                "duration_seconds": command.duration_seconds(),
                "stdout": stdout,
                "stderr": stderr
            })
        })
        .collect::<Vec<_>>();
    let any_command_timed_out = packet.commands().iter().any(|command| command.timed_out());
    let activity_events = packet
        .activity_events()
        .iter()
        .map(|event| {
            json!({
                "kind": "phase",
                "phase": event.phase(),
                "outcome": event.outcome(),
                "started": event.started(),
                "completed": event.completed(),
                "duration_seconds": event.duration_seconds(),
                "detail": event.detail().map(redact_diagnostic)
            })
        })
        .collect();
    let tool_calls = packet
        .tool_calls()
        .iter()
        .map(|tool_call| {
            json!({
                "name": redact_diagnostic(&tool_call.name),
                "arguments": redact_diagnostic(&tool_call.arguments),
                "result": redact_diagnostic(&tool_call.result),
                "outcome": if tool_call.result.trim().is_empty() {"recorded"} else {"completed"}
            })
        })
        .collect();
    Ok(Some(PacketView {
        status,
        acceptance_verdict,
        accepted_revision,
        changed_paths: packet.changed_paths().to_vec(),
        commands,
        artifacts,
        evidence_status: "recorded".into(),
        last_error: packet.last_error().map(|error| redact_diagnostic(error)),
        any_command_timed_out,
        activity_events,
        tool_calls,
        execution_budget_seconds: packet.execution_budget_seconds(),
        deadline_ended_attempt: packet.deadline_ended_attempt().map(str::to_string),
    }))
}

fn unavailable_packet_view(status: &str) -> PacketView {
    PacketView {
        status: "unavailable".into(),
        acceptance_verdict: None,
        accepted_revision: None,
        changed_paths: Vec::new(),
        commands: Vec::new(),
        artifacts: Vec::new(),
        evidence_status: status.into(),
        last_error: None,
        any_command_timed_out: false,
        activity_events: Vec::new(),
        tool_calls: Vec::new(),
        execution_budget_seconds: None,
        deadline_ended_attempt: None,
    }
}

fn command_outcome(succeeded: bool, timed_out: bool) -> &'static str {
    if succeeded {
        "succeeded"
    } else if timed_out {
        "timed_out"
    } else {
        "failed"
    }
}

fn read_review_packet(
    service: &Service,
    invocation: &Invocation,
) -> Result<Option<ReviewPacket>, String> {
    let Some(result) = invocation
        .result
        .as_ref()
        .or(invocation.late_result.as_ref())
    else {
        return Ok(None);
    };
    let Some(packet_path) = result.get("packet_path").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(config) = service.config.workspace.as_ref() else {
        return Ok(None);
    };
    let packet = PathBuf::from(packet_path)
        .canonicalize()
        .map_err(|_| "packet_unavailable".to_string())?;
    let changes_root = config
        .state_root
        .join("state")
        .join("changes")
        .canonicalize()
        .map_err(|_| "packet_unavailable".to_string())?;
    if !packet.starts_with(changes_root) {
        return Err("packet_outside_state_root".into());
    }
    let raw = std::fs::read_to_string(packet).map_err(|_| "packet_unavailable".to_string())?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|_| "packet_decode_failure".to_string())
}

fn output_reference(
    owner: &str,
    invocation_id: &str,
    kind: &str,
    index: usize,
    text: &str,
) -> Value {
    if text.is_empty() {
        return json!({"available":false,"bytes":0,"artifact_id":null,"truncated":false});
    }
    json!({
        "available": true,
        "bytes": text.len(),
        "artifact_id": artifact_id(owner, invocation_id, kind, index),
        "truncated": text.len() > MAX_ARTIFACT_TEXT_BYTES
    })
}

fn artifact_summary(artifact_id: &str, kind: &str, text: &str) -> Value {
    json!({
        "artifact_id": artifact_id,
        "kind": kind,
        "content_type": "text/plain; charset=utf-8",
        "bytes": text.len(),
        "truncated": text.len() > MAX_ARTIFACT_TEXT_BYTES
    })
}

fn artifact_id(owner: &str, invocation_id: &str, kind: &str, index: usize) -> String {
    let token = digest(format!("{owner}\0{invocation_id}\0{kind}\0{index}").as_bytes());
    format!(
        "{ARTIFACT_PREFIX}.{invocation_id}.{kind}.{index}.{}",
        &token[..16]
    )
}

fn parse_artifact_id(value: &str) -> Result<ArtifactKey, String> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 5 || parts[0] != ARTIFACT_PREFIX || !valid_id(parts[1]) {
        return Err("invalid_artifact_id".into());
    }
    if !matches!(parts[2], "stdout" | "stderr")
        || parts[4].len() != 16
        || !parts[4].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("invalid_artifact_id".into());
    }
    let index = parts[3]
        .parse::<usize>()
        .map_err(|_| "invalid_artifact_id".to_string())?;
    Ok(ArtifactKey {
        invocation_id: parts[1].into(),
        kind: parts[2].into(),
        index,
    })
}

fn outcome_category(invocation: &Invocation, packet: Option<&PacketView>) -> &'static str {
    match invocation.state {
        InvocationState::Queued => "pending",
        InvocationState::Running => "running",
        InvocationState::Cancelled => "cancelled",
        InvocationState::Expired => "expired",
        InvocationState::Failed => "failed",
        InvocationState::Uncertain => "uncertain",
        InvocationState::Completed => workspace_category(packet),
    }
}

fn workspace_category(packet: Option<&PacketView>) -> &'static str {
    let Some(packet) = packet else {
        return "succeeded";
    };
    if packet.accepted_revision.is_some() {
        "accepted"
    } else if packet.evidence_status != "recorded" {
        "completed_evidence_unavailable"
    } else if matches!(packet.status.as_str(), "checks_passed" | "prepared") {
        "completed"
    } else if packet.status == "failed" {
        "failed"
    } else {
        "rejected"
    }
}

fn failure_category(invocation: &Invocation, packet: Option<&PacketView>) -> Option<&'static str> {
    match invocation.state {
        InvocationState::Expired if invocation.started.is_some() => Some("execution_timeout"),
        InvocationState::Expired => Some("queue_timeout"),
        InvocationState::Cancelled => Some("cancelled"),
        InvocationState::Failed => Some(match invocation.error.as_deref() {
            Some("backend_response_oversized") => "response_oversized",
            _ => "execution_failed",
        }),
        InvocationState::Uncertain => {
            workspace_failure_category(packet).or(Some("outcome_uncertain"))
        }
        InvocationState::Completed => workspace_failure_category(packet),
        _ => None,
    }
}

fn workspace_failure_category(packet: Option<&PacketView>) -> Option<&'static str> {
    let packet = packet?;
    if packet.deadline_ended_attempt.is_some()
        || packet.any_command_timed_out
        || packet
            .last_error
            .as_deref()
            .is_some_and(is_public_execution_timeout)
    {
        return Some("execution_timeout");
    }
    match packet.status.as_str() {
        "checks_failed" => Some("acceptance_command_failed"),
        "path_policy_failed" => Some("path_policy_failed"),
        "executor_unavailable" => Some("executor_unavailable"),
        "failed" => Some("execution_failed"),
        _ if packet.acceptance_verdict.as_deref() == Some("rejected") => {
            Some("rack_acceptance_rejected")
        }
        _ => None,
    }
}

fn is_public_execution_timeout(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("wall-clock timeout")
        || lower.contains("worker timeout")
        || lower.contains("timeout exceeded")
        || lower.contains("execution timeout")
}

fn model_usage(invocation: &Invocation) -> Value {
    let usage = invocation
        .result
        .as_ref()
        .or(invocation.late_result.as_ref())
        .and_then(|value| value.get("usage"));
    json!({
        "available": usage.is_some(),
        "prompt_tokens": usage.and_then(|u| u.get("prompt_tokens")).and_then(Value::as_u64),
        "completion_tokens": usage.and_then(|u| u.get("completion_tokens")).and_then(Value::as_u64),
        "total_tokens": usage.and_then(|u| u.get("total_tokens")).and_then(Value::as_u64),
        "finish_reason": invocation.result.as_ref().or(invocation.late_result.as_ref())
            .and_then(|value| value.get("choices"))
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(Value::as_str)
    })
}

fn current_activity(invocation: &Invocation) -> &'static str {
    match invocation.state {
        InvocationState::Queued => "queued",
        InvocationState::Running => "running",
        _ => "terminal",
    }
}

fn queue_wait(invocation: &Invocation) -> Option<u64> {
    invocation
        .started
        .zip(optional_time(invocation.created))
        .map(|(started, created)| started.saturating_sub(created))
}

fn active_execution(invocation: &Invocation) -> Option<u64> {
    invocation
        .started
        .zip(invocation.completed)
        .map(|(started, completed)| completed.saturating_sub(started))
}

fn workspace_model_calls(snapshot: &Snapshot) -> Option<u64> {
    snapshot
        .scoped_children
        .as_ref()
        .map(|counts| counts.total as u64)
}

fn workspace_children_availability(snapshot: &Snapshot) -> &'static str {
    if snapshot.scoped_children.is_some() {
        "recorded"
    } else {
        "unavailable"
    }
}

fn packet_availability(packet: Option<&PacketView>) -> &'static str {
    if packet.is_some_and(|packet| packet.evidence_status == "recorded") {
        "recorded"
    } else {
        "unavailable"
    }
}

fn command_errors(packet: &PacketView) -> u64 {
    packet
        .commands
        .iter()
        .filter(|command| command.get("succeeded").and_then(Value::as_bool) == Some(false))
        .count() as u64
}

fn metric_duration(value: Option<u64>) -> Value {
    metric_count(
        value,
        if value.is_some() {
            "recorded"
        } else {
            "unavailable"
        },
    )
}

fn metric_count(value: Option<u64>, availability: &str) -> Value {
    json!({"value": value, "availability": availability})
}

fn optional_time(value: u64) -> Option<u64> {
    (value > 0).then_some(value)
}

fn to_value<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn to_string_value<T: Serialize>(value: T) -> String {
    to_value(value)
        .as_str()
        .unwrap_or("unavailable")
        .to_string()
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(redact_diagnostic(value)),
        Value::Array(values) => Value::Array(values.iter().map(redact_value).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), redact_value(value)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn redact_diagnostic(input: &str) -> String {
    let mut words = Vec::new();
    let mut redact_next_bearer = false;
    for word in input.split_whitespace() {
        if redact_next_bearer {
            words.push("<redacted:credential>".to_string());
            redact_next_bearer = false;
            continue;
        }
        if word.eq_ignore_ascii_case("bearer") {
            words.push("Bearer".to_string());
            redact_next_bearer = true;
            continue;
        }
        words.push(redact_diagnostic_word(word));
    }
    truncate_public_diagnostic(words.join(" "))
}

fn redact_diagnostic_word(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if lower.contains("token=")
        || lower.contains("password=")
        || lower.contains("secret=")
        || lower.contains("capability=")
        || lower.contains("access_key=")
        || lower.contains("authorization:")
    {
        return "<redacted:credential>".to_string();
    }
    if lower.contains("/srv/")
        || lower.contains("/home/")
        || lower.contains("/tmp/")
        || lower.contains("\\srv\\")
        || lower.contains("\\home\\")
        || lower.contains("\\tmp\\")
    {
        return "<redacted:path>".to_string();
    }
    let core = word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric());
    if core.len() >= 32 && core.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return "<redacted:credential>".to_string();
    }
    word.to_string()
}

fn truncate_public_diagnostic(value: String) -> String {
    if value.len() <= MAX_DIAGNOSTIC_TEXT_BYTES {
        return value;
    }
    let mut end = MAX_DIAGNOSTIC_TEXT_BYTES;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = value[..end].to_string();
    truncated.push_str(" <truncated>");
    truncated
}

#[allow(dead_code)]
fn safe_relative(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{collections::BTreeMap, path::PathBuf};

    fn workspace_work() -> crate::work_payload::Work {
        serde_json::from_value(json!({
            "reservation_id": "reservation",
            "service": "local-coder",
            "work_id": "work",
            "payload": {
                "kind": "workspace",
                "workspace": {
                    "repository": {"id": "repo", "base_ref": "main"},
                    "objective": "Make one bounded change.",
                    "allowed_paths": ["src/"],
                    "acceptance": {"commands": [["cargo", "test"]]},
                    "requirements": {"complexity": "small", "requires_large_context": false},
                    "limits": {"max_implementation_attempts": 1, "timeout_seconds": 30}
                }
            }
        }))
        .unwrap()
    }

    fn invocation(state: InvocationState, error: Option<&str>) -> Invocation {
        Invocation {
            scope_access_hash: None,
            id: "invocation".into(),
            owner: "athba".into(),
            request: Inference {
                work: Some(workspace_work()),
                schema: VERSION.into(),
                submission_id: "submission".into(),
                reservation_id: "reservation".into(),
                generation: "generation".into(),
                profile_hash: "profile".into(),
                prompt: String::new(),
                payload: None,
                max_tokens: 1,
                timeout_seconds: 30,
                wait_seconds: None,
                workspace_scope: None,
            },
            request_digest: None,
            request_bytes: None,
            work_digest: None,
            work_bytes: None,
            request_ref: None,
            state,
            queue_order: 7,
            created: 10,
            waiting_deadline: 70,
            execution_deadline: Some(100),
            response_bytes: 1024,
            cancellation: None,
            late_result: None,
            result_digest: None,
            late_result_digest: None,
            result_ref: None,
            late_result_ref: None,
            started: Some(12),
            completed: Some(18),
            activation: Some("activation".into()),
            result: None,
            error: error.map(str::to_string),
        }
    }

    fn children(total: usize, unresolved: usize) -> ScopedCounts {
        ScopedCounts {
            total,
            unresolved,
            children: Vec::new(),
        }
    }

    fn demand(state: DemandState) -> Demand {
        Demand {
            reservation_id: Some("reservation".into()),
            services: BTreeMap::new(),
            reserve_request: None,
            reserve_result: None,
            reservation_closed: None,
            recovery_reconciliation: None,
            recovery_error: None,
            workspace_recovery_analyses: BTreeMap::new(),
            id: "reservation".into(),
            owner: "athba".into(),
            request: Acquire {
                schema: VERSION.into(),
                source_system: "athba".into(),
                work_id: "work".into(),
                acquisition_id: "acquisition".into(),
                tag: "local-coder".into(),
                priority: Some(Priority::Low),
                capabilities: Vec::new(),
                context_tokens: 4096,
                ttl_seconds: 60,
                qualification: false,
            },
            priority: Priority::Low,
            profile: crate::config::Profile {
                tag: "local-coder".into(),
                version: "fixture".into(),
                model: "fixture".into(),
                backend: crate::config::Backend::Vllm,
                driver: crate::config::Driver::Fixture,
                qualified: true,
                evidence: Vec::new(),
                capabilities: Vec::new(),
                context_tokens: 4096,
                max_input_tokens: 3968,
                protocols: crate::protocol::default_protocols(),
                streaming: false,
                max_images_per_request: 0,
                max_image_bytes: 0,
                max_image_pixels: 0,
                max_output_tokens: 128,
                resources: Vec::new(),
                device_mib: BTreeMap::new(),
                host_mib: 0,
                cpu_percent: 0,
                endpoint: "http://127.0.0.1:1".into(),
                executable: PathBuf::from("/bin/true"),
                container_image: None,
                container_mounts: BTreeMap::new(),
                executable_sha256: "0".repeat(64),
                args: Vec::new(),
                media_config: None,
                media_config_sha256: None,
                media_mode: None,
                artifact: None,
                artifact_sha256: None,
                artifact_verify_seconds: 1,
                startup_seconds: 1,
                drain_seconds: 1,
                stop_seconds: 1,
                inference_seconds: 1,
            },
            profile_hash: "profile".into(),
            state,
            reason: None,
            retry_after: None,
            preempted_by: None,
            ready_checked: true,
            accepted_calls: 0,
            generation: "generation".into(),
            access_key: "access".into(),
            backend_activation: None,
            created: 10,
            last_activity_at: None,
            order: 0,
            deadline: 70,
            transition_deadline: 80,
            victims: Vec::new(),
            process: None,
            effect_started: false,
            preflight_done: true,
            released: matches!(state, DemandState::Released),
        }
    }

    fn packet(accepted_revision: Option<&str>, status: &str, verdict: Option<&str>) -> PacketView {
        PacketView {
            status: status.into(),
            acceptance_verdict: verdict.map(str::to_string),
            accepted_revision: accepted_revision.map(str::to_string),
            changed_paths: vec!["src/lib.rs".into()],
            commands: vec![json!({
                "index": 0,
                "argv": ["cargo", "test"],
                "exit_code": 0,
                "timed_out": false,
                "succeeded": true,
                "outcome": "succeeded",
                "started": 13,
                "completed": 14,
                "duration_seconds": 1,
                "stdout": {"available": true, "bytes": 4, "artifact_id": artifact_id("athba", "invocation", "stdout", 0), "truncated": false},
                "stderr": {"available": false, "bytes": 0, "artifact_id": null, "truncated": false}
            })],
            artifacts: vec![artifact_summary(
                &artifact_id("athba", "invocation", "stdout", 0),
                "command_stdout",
                "pass",
            )],
            evidence_status: "recorded".into(),
            last_error: None,
            any_command_timed_out: false,
            activity_events: vec![json!({
                "kind": "phase",
                "phase": "setup",
                "outcome": "completed",
                "started": 12,
                "completed": 13,
                "duration_seconds": 1,
                "detail": null
            })],
            tool_calls: Vec::new(),
            execution_budget_seconds: Some(30),
            deadline_ended_attempt: None,
        }
    }

    #[test]
    fn workspace_report_projects_accepted_packet_without_private_paths() {
        let snapshot = Snapshot {
            invocation: invocation(InvocationState::Completed, None),
            demand: None,
            archived: false,
            scoped_children: Some(children(2, 0)),
        };
        let value = report(
            "athba",
            &snapshot,
            Some(&packet(Some("abc123"), "checks_passed", Some("approved"))),
        );
        assert_eq!(value["contract_version"], "1.4.0");
        assert_eq!(value["outcome"]["kind"], "workspace");
        assert_eq!(value["outcome"]["category"], "accepted");
        assert_eq!(value["outcome"]["attempt"]["known"], true);
        assert_eq!(value["outcome"]["workspace"]["accepted_revision"], "abc123");
        assert_eq!(value["activity"]["counts"]["model_calls"]["value"], 2);
        assert_eq!(value["activity"]["timings"]["setup_seconds"]["value"], 1);
        let rendered = value.to_string();
        assert!(!rendered.contains("packet_path"));
        assert!(!rendered.contains("worktree_path"));
        assert!(!rendered.contains("/srv/"));
        assert!(rendered.contains("wa1.invocation.stdout.0."));
    }

    #[test]
    fn confirmed_agent_timeout_is_reported_without_rewriting_uncertain_history() {
        let mut timeout_packet = packet(None, "failed", None);
        timeout_packet.last_error =
            Some("JCode wall-clock timeout exceeded in /srv/rack-ai/private".into());
        timeout_packet.deadline_ended_attempt = Some("agent_execution".into());
        timeout_packet.activity_events = vec![json!({
            "kind": "phase",
            "phase": "agent_execution",
            "outcome": "failed",
            "started": 13,
            "completed": 43,
            "duration_seconds": 30,
            "detail": "JCode wall-clock timeout exceeded"
        })];
        let snapshot = Snapshot {
            invocation: invocation(
                InvocationState::Uncertain,
                Some("workspace_model_outcome_uncertain"),
            ),
            demand: Some(demand(DemandState::Releasing)),
            archived: false,
            scoped_children: Some(children(1, 0)),
        };

        let view = report("athba", &snapshot, Some(&timeout_packet));

        assert_eq!(view["outcome"]["category"], "uncertain");
        assert_eq!(view["outcome"]["outcome_known"], false);
        assert_eq!(view["outcome"]["failure_category"], "execution_timeout");
        assert_eq!(view["outcome"]["attempt"]["known"], true);
        assert_eq!(view["outcome"]["attempt"]["category"], "failed");
        assert_eq!(
            view["outcome"]["attempt"]["failure_category"],
            "execution_timeout"
        );
        assert_eq!(
            view["outcome"]["attempt"]["deadline_ended_attempt"],
            "agent_execution"
        );
        assert_eq!(
            view["outcome"]["historical_invocation"]["state"],
            "uncertain"
        );
        assert_eq!(
            view["outcome"]["historical_invocation"]["outcome_known"],
            false
        );
        assert_eq!(view["closure"]["safe_closure_known"], false);
        assert_eq!(
            view["closure"]["replay_safety"],
            "blocked_unresolved_effect"
        );
        assert_eq!(
            view["activity"]["timings"]["agent_execution_seconds"]["value"],
            30
        );
        assert!(!view.to_string().contains("/srv/rack-ai/private"));
    }

    #[test]
    fn safe_closure_can_be_known_while_historical_uncertainty_remains() {
        let snapshot = Snapshot {
            invocation: invocation(
                InvocationState::Uncertain,
                Some("workspace_model_outcome_uncertain"),
            ),
            demand: Some(demand(DemandState::Released)),
            archived: false,
            scoped_children: None,
        };
        let view = report("athba", &snapshot, Some(&packet(None, "failed", None)));
        assert_eq!(view["outcome"]["category"], "uncertain");
        assert_eq!(
            view["outcome"]["historical_invocation"]["state"],
            "uncertain"
        );
        assert_eq!(view["closure"]["cleanup_state"], "safely_closed");
        assert_eq!(view["closure"]["safe_closure_known"], true);
        assert_eq!(view["closure"]["replay_safety"], "closed_inspection_only");
    }

    #[test]
    fn truly_unknown_and_active_work_do_not_become_safe_replay() {
        let uncertain = Snapshot {
            invocation: invocation(
                InvocationState::Uncertain,
                Some("workspace_model_outcome_uncertain"),
            ),
            demand: None,
            archived: false,
            scoped_children: Some(children(1, 1)),
        };
        let unknown = report("athba", &uncertain, None);
        assert_eq!(unknown["outcome"]["attempt"]["known"], false);
        assert_eq!(unknown["outcome"]["attempt"]["category"], "unknown");
        assert_eq!(unknown["closure"]["safe_closure_known"], false);

        let active = Snapshot {
            invocation: invocation(InvocationState::Running, None),
            demand: None,
            archived: false,
            scoped_children: Some(children(0, 0)),
        };
        let view = report("athba", &active, None);
        assert_eq!(view["closure"]["execution_active"], true);
        assert_eq!(view["closure"]["safe_closure_known"], false);
        assert_eq!(view["closure"]["replay_safety"], "blocked_active_execution");
    }

    #[test]
    fn diagnostic_projection_redacts_paths_and_credentials() {
        let mut packet = packet(None, "failed", None);
        let secret = "abcd1234abcd1234abcd1234abcd1234";
        packet.last_error = Some(format!(
            "failed at /srv/rack-ai/state token={secret} Bearer {secret}"
        ));
        packet.commands = vec![json!({
            "index": 0,
            "argv": ["/srv/rack-ai/bin/tool", format!("access_key={secret}")],
            "exit_code": 1,
            "timed_out": false,
            "succeeded": false,
            "outcome": "failed",
            "started": null,
            "completed": null,
            "duration_seconds": null,
            "stdout": {"available": true, "bytes": 16, "artifact_id": artifact_id("athba", "invocation", "stdout", 0), "truncated": false},
            "stderr": {"available": true, "bytes": 16, "artifact_id": artifact_id("athba", "invocation", "stderr", 0), "truncated": false}
        })];
        packet.artifacts = vec![artifact_summary(
            &artifact_id("athba", "invocation", "stdout", 0),
            "command_stdout",
            "<redacted:path>",
        )];
        packet.activity_events = vec![json!({
            "kind": "phase",
            "phase": "agent_execution",
            "outcome": "failed",
            "started": 12,
            "completed": 13,
            "duration_seconds": 1,
            "detail": format!("/home/tomp/private token={secret}")
        })];
        packet.tool_calls = vec![json!({
            "name": "shell",
            "arguments": format!("cat /tmp/private capability={secret}"),
            "result": format!("Bearer {secret}")
        })];

        let snapshot = Snapshot {
            invocation: invocation(InvocationState::Completed, None),
            demand: None,
            archived: false,
            scoped_children: Some(children(0, 0)),
        };
        let rendered = report("athba", &snapshot, Some(&packet)).to_string();
        assert!(!rendered.contains("/srv/"));
        assert!(!rendered.contains("/home/tomp"));
        assert!(!rendered.contains("/tmp/"));
        assert!(!rendered.contains(secret));
        assert!(rendered.contains("<redacted:path>"));
        assert!(rendered.contains("<redacted:credential>"));
    }

    #[test]
    fn missing_optional_metrics_are_explicitly_unavailable() {
        let snapshot = Snapshot {
            invocation: invocation(InvocationState::Completed, None),
            demand: None,
            archived: false,
            scoped_children: None,
        };
        let view = report("athba", &snapshot, None);
        assert_eq!(
            view["activity"]["timings"]["first_token_seconds"]["availability"],
            "unavailable"
        );
        assert_eq!(
            view["activity"]["timings"]["prefill_seconds"]["availability"],
            "unavailable"
        );
        assert_eq!(
            view["activity"]["counts"]["scoped_children"]["availability"],
            "unavailable"
        );
    }

    #[test]
    fn artifact_ids_are_owner_bound_and_opaque() {
        let athba = artifact_id("athba", "invocation", "stdout", 0);
        let cb = artifact_id("cb", "invocation", "stdout", 0);
        assert_ne!(athba, cb);
        let parsed = parse_artifact_id(&athba).unwrap();
        assert_eq!(parsed.invocation_id, "invocation");
        assert_eq!(parsed.kind, "stdout");
        assert_eq!(parsed.index, 0);
        assert!(parse_artifact_id("/srv/rack-ai/state/changes/packet.json").is_err());
    }
}
