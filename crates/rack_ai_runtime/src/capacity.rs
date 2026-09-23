//! Queue capacity is a reservation contract. Terminal evidence is compacted to
//! digests before it can consume active-call admission headroom.
use crate::types::*;
use serde::Deserialize;

pub const API_REQUEST_BODY_BYTES: u64 = 1024 * 1024;
const CONTROL_COMPLETION_HEADROOM_BYTES: u64 = 64 * 1024;
const PAYLOAD_COMPLETION_HEADROOM_BYTES: u64 = 64 * 1024;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_pending: usize,
    pub max_pending_per_reservation: usize,
    pub max_calls_per_reservation: u64,
    pub max_dispatch_workers: usize,
    pub max_transition_workers: usize,
    pub max_gateway_waiters: usize,
    pub max_wait_seconds: u64,
    pub max_response_bytes: u64,
    /// Bound for active queue reservations plus compact control data.
    pub retention_admission_bytes: u64,
    /// Full terminal payloads are retained only within this bounded window;
    /// older terminal payloads retain a durable SHA-256 receipt instead.
    pub terminal_evidence_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pending: 32,
            max_pending_per_reservation: 16,
            max_calls_per_reservation: 256,
            max_dispatch_workers: 4,
            max_transition_workers: 8,
            max_gateway_waiters: 32,
            max_wait_seconds: 300,
            max_response_bytes: 3 * 1024 * 1024,
            retention_admission_bytes: 30 * 1024 * 1024,
            terminal_evidence_bytes: 8 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_pending == 0
            || self.max_pending > 1024
            || self.max_pending_per_reservation == 0
            || self.max_pending_per_reservation > self.max_pending
            || self.max_calls_per_reservation < self.max_pending_per_reservation as u64
            || self.max_calls_per_reservation > 16384
            || self.max_dispatch_workers == 0
            || self.max_dispatch_workers > 64
            || self.max_transition_workers == 0
            || self.max_transition_workers > 64
            || self.max_gateway_waiters == 0
            || self.max_gateway_waiters > 128
            || self.max_wait_seconds == 0
            || self.max_wait_seconds > 86400
            || !(1024..=4 * 1024 * 1024).contains(&self.max_response_bytes)
            || !(256 * 1024..=30 * 1024 * 1024).contains(&self.retention_admission_bytes)
            || !(64 * 1024..=30 * 1024 * 1024).contains(&self.terminal_evidence_bytes)
        {
            return Err("invalid_runtime_capacity_limits".into());
        }
        Ok(())
    }

    pub fn pending(&self, s: &Document, reservation: &str) -> Result<(), String> {
        let pending: Vec<_> = s
            .data
            .invocations
            .values()
            .filter(|i| matches!(i.state, InvocationState::Queued | InvocationState::Running))
            .collect();
        if pending.len() >= self.max_pending {
            return Err("capacity_pending_global".into());
        }
        if pending
            .iter()
            .filter(|i| i.request.reservation_id == reservation)
            .count()
            >= self.max_pending_per_reservation
        {
            return Err("capacity_pending_reservation".into());
        }
        let demand = s
            .data
            .demands
            .get(reservation)
            .ok_or("missing_reservation")?;
        if demand.accepted_calls >= self.max_calls_per_reservation {
            return Err("capacity_reservation_call_history".into());
        }
        Ok(())
    }
}

/// Compact terminal evidence before it can consume active-call admission
/// headroom. Active queued/running work is never compacted.
pub fn retention(s: &mut Document, limits: &Limits) -> Result<(), String> {
    compact_terminal_requests(s)?;
    compact_terminal_payloads(
        s,
        limits
            .terminal_evidence_bytes
            .min(limits.retention_admission_bytes / 2),
    )?;
    let control_reserved = serde_json::to_vec(s).map_err(|e| e.to_string())?.len() as u64;
    if control_reserved.saturating_add(CONTROL_COMPLETION_HEADROOM_BYTES)
        > limits.retention_admission_bytes
    {
        return Err("capacity_active_control".into());
    }
    let payload_reserved = s
        .data
        .invocations
        .values()
        .filter(|invocation| {
            matches!(
                invocation.state,
                InvocationState::Queued | InvocationState::Running
            )
        })
        .map(crate::payload_store::active_payload_commitment)
        .sum::<u64>();
    if payload_reserved.saturating_add(PAYLOAD_COMPLETION_HEADROOM_BYTES)
        > active_payload_capacity(limits)
    {
        return Err("capacity_active_payload".into());
    }
    Ok(())
}

pub fn active_payload_capacity(limits: &Limits) -> u64 {
    let per_pending = API_REQUEST_BODY_BYTES.saturating_add(
        crate::payload_store::response_storage_bound(limits.max_response_bytes),
    );
    (limits.max_pending as u64)
        .saturating_mul(per_pending)
        .saturating_add((limits.max_dispatch_workers as u64) * PAYLOAD_COMPLETION_HEADROOM_BYTES)
}

fn compact_terminal_requests(s: &mut Document) -> Result<(), String> {
    for invocation in s.data.invocations.values_mut() {
        if !matches!(
            invocation.state,
            InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
        ) {
            compact_terminal_request(invocation)?;
        }
    }
    Ok(())
}

pub(crate) fn compact_terminal_request(invocation: &mut Invocation) -> Result<(), String> {
    let original = json_bytes(&invocation.request)?;
    let mut compacted = invocation.request.clone();
    compacted.prompt.clear();
    compacted.payload = None;
    if let Some(work) = compacted.work.as_mut() {
        compact_work_request(work);
    }
    let compacted_bytes = json_bytes(&compacted)?.len() as u64;
    if compacted_bytes >= original.len() as u64 {
        return Ok(());
    }
    invocation
        .request_digest
        .get_or_insert_with(|| digest(&original));
    invocation
        .request_bytes
        .get_or_insert(original.len() as u64);
    if let Some(work) = invocation.request.work.as_ref() {
        let work_bytes = json_bytes(work)?;
        invocation
            .work_digest
            .get_or_insert_with(|| digest(&work_bytes));
        invocation.work_bytes.get_or_insert(work_bytes.len() as u64);
    }
    invocation.request = compacted;
    Ok(())
}

fn compact_work_request(work: &mut crate::work_payload::Work) {
    match &mut work.payload {
        crate::work_payload::Payload::Inference { prompt, .. } => prompt.clear(),
        crate::work_payload::Payload::Workspace { workspace } => workspace.objective.clear(),
    }
}

fn compact_terminal_payloads(s: &mut Document, limit: u64) -> Result<(), String> {
    let mut entries = Vec::new();
    for (id, invocation) in &s.data.invocations {
        if !matches!(
            invocation.state,
            InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
        ) {
            let result = invocation
                .result
                .as_ref()
                .map(serialized_bytes)
                .transpose()?
                .unwrap_or(0);
            let late = invocation
                .late_result
                .as_ref()
                .map(serialized_bytes)
                .transpose()?
                .unwrap_or(0);
            entries.push((invocation.started.unwrap_or(0), id.clone(), result + late));
        }
    }
    let mut total = entries.iter().map(|(_, _, bytes)| *bytes).sum::<u64>();
    if total <= limit {
        return Ok(());
    }
    entries.sort_by_key(|(started, id, _)| (*started, id.clone()));
    for (_, id, _) in entries {
        if total <= limit {
            break;
        }
        let invocation = s
            .data
            .invocations
            .get_mut(&id)
            .ok_or("missing_invocation")?;
        if let Some(value) = invocation.result.take() {
            let bytes = serialized_bytes(&value)?;
            total = total.saturating_sub(bytes);
            invocation.result_digest = Some(digest(
                &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
            ));
        }
        if total <= limit {
            break;
        }
        if let Some(value) = invocation.late_result.take() {
            let bytes = serialized_bytes(&value)?;
            total = total.saturating_sub(bytes);
            invocation.late_result_digest = Some(digest(
                &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
            ));
        }
    }
    Ok(())
}

fn serialized_bytes(value: &serde_json::Value) -> Result<u64, String> {
    Ok(serde_json::to_vec(value).map_err(|e| e.to_string())?.len() as u64)
}

pub fn diagnostic(mut error: String) -> String {
    let mut end = error.len().min(2048);
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    error.truncate(end);
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_response_budget_baseline_is_three_mib() {
        const BASELINE: u64 = 3 * 1024 * 1024;
        assert_eq!(Limits::default().max_response_bytes, BASELINE);
        let example: serde_json::Value =
            serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                .unwrap();
        assert_eq!(
            example["limits"]["max_response_bytes"].as_u64(),
            Some(BASELINE)
        );
    }

    #[test]
    fn configured_capacity_limits_reject_zero_excess_and_unknown_fields() {
        for value in [
            serde_json::json!({"max_pending":0}),
            serde_json::json!({"max_pending_per_reservation":33}),
            serde_json::json!({"max_calls_per_reservation":1}),
            serde_json::json!({"max_dispatch_workers":0}),
            serde_json::json!({"max_transition_workers":0}),
            serde_json::json!({"max_gateway_waiters":129}),
            serde_json::json!({"max_wait_seconds":0}),
            serde_json::json!({"max_response_bytes":4194305}),
            serde_json::json!({"retention_admission_bytes":33554432}),
            serde_json::json!({"terminal_evidence_bytes":1}),
        ] {
            assert!(
                serde_json::from_value::<Limits>(value)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        assert!(serde_json::from_value::<Limits>(serde_json::json!({"unbounded":true})).is_err());
        assert!(Limits::default().validate().is_ok());
    }

    use crate::{
        config::{Config, Driver},
        inference::Submission,
        protocol::{Payload as ProtocolPayload, Protocol},
        service::Service,
    };
    use serde_json::{Value, json};
    use std::{collections::BTreeMap, fs};

    fn empty_document() -> Document {
        Document {
            claims: BTreeMap::new(),
            data: State::default(),
        }
    }

    fn test_limits() -> Limits {
        Limits {
            max_response_bytes: 1024 * 1024,
            ..Limits::default()
        }
    }

    fn protocol_request(
        submission_id: &str,
        reservation_id: &str,
        model: &str,
        bytes: usize,
    ) -> Inference {
        Inference {
            work: None,
            schema: VERSION.into(),
            submission_id: submission_id.into(),
            reservation_id: reservation_id.into(),
            generation: "generation".into(),
            profile_hash: "profile-hash".into(),
            prompt: "prompt-prefix".into(),
            payload: Some(ProtocolPayload {
                protocol: Protocol::ChatCompletions,
                body: json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "x".repeat(bytes)}],
                    "max_tokens": 1,
                    "stream": false
                }),
            }),
            max_tokens: 1,
            timeout_seconds: 30,
            wait_seconds: Some(30),
            workspace_scope: None,
        }
    }

    fn completed_invocation(id: &str, request: Inference) -> Invocation {
        invocation(id, InvocationState::Completed, request, 1024)
    }

    fn invocation(
        id: &str,
        state: InvocationState,
        request: Inference,
        response_bytes: u64,
    ) -> Invocation {
        Invocation {
            scope_access_hash: None,
            id: id.into(),
            owner: "cb".into(),
            request,
            request_digest: None,
            request_bytes: None,
            work_digest: None,
            work_bytes: None,
            request_ref: None,
            state,
            queue_order: 1,
            waiting_deadline: now() + 60,
            execution_deadline: None,
            response_bytes,
            cancellation: None,
            late_result: None,
            result_digest: None,
            late_result_digest: None,
            result_ref: None,
            late_result_ref: None,
            started: Some(now()),
            activation: Some("generation".into()),
            result: Some(json!({"ok": true})),
            error: None,
        }
    }

    fn terminal_request_bytes(s: &Document) -> u64 {
        s.data
            .invocations
            .values()
            .filter(|i| {
                !matches!(
                    i.state,
                    InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
                )
            })
            .map(|i| json_bytes(&i.request).unwrap().len() as u64)
            .sum()
    }

    fn active_payload_commitment_bytes(s: &Document) -> u64 {
        s.data
            .invocations
            .values()
            .filter(|i| matches!(i.state, InvocationState::Queued | InvocationState::Running))
            .map(crate::payload_store::active_payload_commitment)
            .sum()
    }

    fn workspace_work(objective_bytes: usize) -> crate::work_payload::Work {
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
                    "objective": "z".repeat(objective_bytes),
                    "allowed_paths": ["tests/test_running_total.py"],
                    "acceptance": {"commands": [["pytest", "tests/test_running_total.py"]]},
                    "requirements": {"complexity": "medium", "requires_large_context": false},
                    "limits": {"max_implementation_attempts": 1, "timeout_seconds": 300}
                }
            },
            "wait_seconds": 30
        }))
        .unwrap()
    }

    #[test]
    fn terminal_request_history_compacts_old_representation_and_admits_new_work() {
        let limits = test_limits();
        let mut document = empty_document();
        for index in 0..260 {
            let request = protocol_request(
                &format!("terminal-{index}"),
                "reservation",
                "local-primary",
                140 * 1024,
            );
            document.data.invocations.insert(
                format!("terminal-{index:03}"),
                completed_invocation(&format!("terminal-{index:03}"), request),
            );
        }
        let queued = invocation(
            "queued",
            InvocationState::Queued,
            protocol_request("queued", "reservation", "local-primary", 128 * 1024),
            limits.max_response_bytes,
        );
        document.data.invocations.insert("queued".into(), queued);

        let old_json: Value = serde_json::to_value(&document).unwrap();
        assert!(
            old_json["data"]["invocations"]["terminal-000"]
                .get("request_digest")
                .is_none()
        );
        let mut document: Document = serde_json::from_value(old_json).unwrap();
        let before_terminal_requests = terminal_request_bytes(&document);
        let before_control = json_bytes(&document).unwrap().len() as u64;
        assert!(before_control > limits.retention_admission_bytes);

        retention(&mut document, &limits).unwrap();

        let after_terminal_requests = terminal_request_bytes(&document);
        let after_control = json_bytes(&document).unwrap().len() as u64;
        let active_payload = active_payload_commitment_bytes(&document);
        assert!(after_terminal_requests < before_terminal_requests / 10);
        assert!(
            after_control.saturating_add(CONTROL_COMPLETION_HEADROOM_BYTES)
                <= limits.retention_admission_bytes
        );
        assert!(
            active_payload.saturating_add(PAYLOAD_COMPLETION_HEADROOM_BYTES)
                <= active_payload_capacity(&limits)
        );
        let terminal = document.data.invocations.get("terminal-000").unwrap();
        assert!(terminal.request_digest.is_some());
        assert!(
            terminal.request_bytes.unwrap() > json_bytes(&terminal.request).unwrap().len() as u64
        );
        assert!(terminal.request.payload.is_none());
        assert!(terminal.request.prompt.is_empty());
        let queued = document.data.invocations.get("queued").unwrap();
        assert!(queued.request.payload.is_some());
        assert_eq!(queued.response_bytes, limits.max_response_bytes);
        assert!(queued.request_digest.is_none());
    }

    #[test]
    fn active_and_unresolved_uncertain_requests_are_not_compacted() {
        let limits = test_limits();
        let mut document = empty_document();
        for (id, state) in [
            ("queued", InvocationState::Queued),
            ("running", InvocationState::Running),
            ("uncertain", InvocationState::Uncertain),
        ] {
            document.data.invocations.insert(
                id.into(),
                invocation(
                    id,
                    state,
                    protocol_request(id, "reservation", "local-primary", 64 * 1024),
                    64 * 1024,
                ),
            );
        }

        retention(&mut document, &limits).unwrap();

        for id in ["queued", "running", "uncertain"] {
            let saved = document.data.invocations.get(id).unwrap();
            assert!(saved.request.payload.is_some());
            assert_eq!(saved.request.prompt, "prompt-prefix");
            assert!(saved.request_digest.is_none());
        }
    }

    #[test]
    fn compacted_workspace_work_retains_digest_identity() {
        let limits = test_limits();
        let work = workspace_work(128 * 1024);
        let mut document = empty_document();
        document.data.invocations.insert(
            "workspace".into(),
            completed_invocation(
                "workspace",
                Inference {
                    work: Some(work.clone()),
                    schema: VERSION.into(),
                    submission_id: "work-submission".into(),
                    reservation_id: "reservation".into(),
                    generation: "generation".into(),
                    profile_hash: "profile-hash".into(),
                    prompt: String::new(),
                    payload: None,
                    max_tokens: 1,
                    timeout_seconds: 300,
                    wait_seconds: Some(30),
                    workspace_scope: None,
                },
            ),
        );

        retention(&mut document, &limits).unwrap();

        let saved = document.data.invocations.get("workspace").unwrap();
        assert!(saved.work_digest.is_some());
        assert!(crate::inference::work_matches(saved, &work).unwrap());
        let mut changed = work.clone();
        if let crate::work_payload::Payload::Workspace { workspace } = &mut changed.payload {
            workspace.objective.push_str("changed");
        }
        assert!(!crate::inference::work_matches(saved, &changed).unwrap());
        let saved_work = saved.request.work.as_ref().unwrap();
        if let crate::work_payload::Payload::Workspace { workspace } = &saved_work.payload {
            assert!(workspace.objective.is_empty());
            assert_eq!(workspace.repository.id, "repo");
            assert_eq!(workspace.allowed_paths, vec!["tests/test_running_total.py"]);
        } else {
            panic!("workspace payload expected");
        }
    }

    fn fixture_service() -> (Service, Config) {
        let mut config: Config =
            serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                .unwrap();
        config.authority_root =
            std::env::temp_dir().join(format!("rack-retention-{}", identity().unwrap()));
        fs::create_dir_all(&config.authority_root).unwrap();
        config.fixture_mode = true;
        for profile in &mut config.profiles {
            profile.driver = Driver::Fixture;
            profile.qualified = true;
            profile.evidence = vec!["synthetic-no-gpu".into()];
        }
        (Service::new(config.clone()), config)
    }

    fn ready_demand(service: &Service) -> Demand {
        let profile = service
            .config
            .profiles
            .iter()
            .find(|profile| profile.tag == "local-primary")
            .unwrap()
            .clone();
        let demand = Demand {
            reservation_id: None,
            services: BTreeMap::new(),
            reserve_request: None,
            reserve_result: None,
            reservation_closed: None,
            recovery_reconciliation: None,
            recovery_error: None,
            workspace_recovery_analyses: BTreeMap::new(),
            id: "reservation".into(),
            owner: "cb".into(),
            request: Acquire {
                schema: VERSION.into(),
                source_system: "cb".into(),
                work_id: "reservation-work".into(),
                acquisition_id: "acquire".into(),
                tag: profile.tag.clone(),
                priority: Some(Priority::Low),
                capabilities: profile.capabilities.clone(),
                context_tokens: profile.context_tokens,
                ttl_seconds: 3600,
                qualification: false,
            },
            priority: Priority::Low,
            profile,
            profile_hash: "profile-hash".into(),
            state: DemandState::Ready,
            reason: None,
            retry_after: None,
            preempted_by: None,
            ready_checked: true,
            accepted_calls: 0,
            generation: "generation".into(),
            access_key: "access-key".into(),
            backend_activation: Some("activation".into()),
            created: now(),
            last_activity_at: None,
            order: 1,
            deadline: now() + 3600,
            transition_deadline: now() + 60,
            victims: Vec::new(),
            process: None,
            effect_started: true,
            preflight_done: true,
            released: false,
        };
        service
            .authority
            .update(|s| {
                for resource in &demand.profile.resources {
                    s.claims.insert(resource.clone(), demand.id.clone());
                }
                s.data.demands.insert(demand.id.clone(), demand.clone());
                Ok(())
            })
            .unwrap();
        demand
    }

    #[test]
    fn compacted_terminal_request_idempotency_and_conflict_survive_reload() {
        let (service, config) = fixture_service();
        let demand = ready_demand(&service);
        let request = protocol_request("stable", &demand.id, &demand.profile.model, 128 * 1024);
        let invocation = Submission { service: &service }
            .submit(&demand.owner, request.clone())
            .unwrap();
        service
            .authority
            .update(|s| {
                let saved = s.data.invocations.get_mut(&invocation.id).unwrap();
                saved.state = InvocationState::Completed;
                saved.result = Some(json!({"rack_protocol_response": {"content_type": "application/json", "body": "{}"}}));
                retention(s, &service.config.limits)?;
                Ok(())
            })
            .unwrap();
        service
            .authority
            .read(|s| {
                let saved = s.data.invocations.get(&invocation.id).unwrap();
                assert!(saved.request_digest.is_some());
                assert!(saved.request.payload.is_none());
                Ok(())
            })
            .unwrap();

        let reloaded = Service::new(config.clone());
        assert_eq!(
            Submission { service: &reloaded }
                .submit(&demand.owner, request.clone())
                .unwrap()
                .id,
            invocation.id
        );
        let mut changed = request;
        let body = &mut changed.payload.as_mut().unwrap().body;
        body["messages"][0]["content"] = json!("different request");
        assert_eq!(
            Submission { service: &reloaded }
                .submit(&demand.owner, changed)
                .unwrap_err(),
            "identity_conflict"
        );
        fs::remove_dir_all(&config.authority_root).unwrap();
    }
}
