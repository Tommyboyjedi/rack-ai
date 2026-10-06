use crate::{
    admission::Admission, config::Config, interaction_capture, interaction_diagnostics::Limits,
    interaction_store as store, service::Service, types::*,
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt};

struct Fixture {
    service: Service,
    demand: Demand,
    parent: Invocation,
    child: Invocation,
}
impl Fixture {
    fn new(enabled: bool) -> Self {
        let mut config: Config =
            serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                .unwrap();
        config.authority_root =
            std::env::temp_dir().join(format!("rack-interactions-{}", identity().unwrap()));
        let service = Service::new(config);
        let source = &service.config.sources[0];
        let profile = &service.config.profiles[0];
        let mut demand = (Admission {
            service: &service,
            source,
        })
        .prepare(
            Acquire {
                schema: VERSION.into(),
                source_system: source.source.clone(),
                work_id: "generic-work".into(),
                acquisition_id: "dev-acquisition".into(),
                tag: profile.tag.clone(),
                priority: Some(Priority::Low),
                capabilities: profile.capabilities.clone(),
                context_tokens: profile.context_tokens,
                ttl_seconds: 60,
                qualification: false,
            },
            0,
        )
        .unwrap();
        demand.state = DemandState::Ready;
        demand.reservation_id = Some(demand.id.clone());
        demand.reserve_request = Some(crate::reservation::Reserve {
            acquisition_id: "dev-acquisition".into(),
            work_id: "generic-work".into(),
            services: vec![profile.tag.clone()],
            priority: Priority::Low,
            ttl_seconds: 60,
            diagnostics: crate::reservation::Diagnostics {
                retained_model_interactions: enabled,
            },
        });
        let parent: Invocation = serde_json::from_value(json!({
            "id":"parent","owner":demand.owner,"state":"running","created":now(),"started":now(),
            "waiting_deadline":now()+60,"execution_deadline":now()+60,"activation":demand.generation,
            "response_bytes":3145728,"cancellation":null,"late_result":null,"result":null,"error":null,
            "request":{"schema":VERSION,"submission_id":"parent","reservation_id":demand.id,
                "generation":demand.generation,"profile_hash":demand.profile_hash,
                "prompt":"","max_tokens":16,"timeout_seconds":30,
                "work":{"reservation_id":demand.id,"service":demand.profile.tag,"work_id":"generic-work",
                    "payload":{"kind":"inference","prompt":"parent","max_tokens":16,"timeout_seconds":30}}}
        })).unwrap();
        let mut child = parent.clone();
        child.id = "child-1".into();
        child.request.work = None;
        child.request.workspace_scope = Some("scope".into());
        child.queue_order = 1;
        service
            .authority
            .update(|s| {
                s.data.demands.insert(demand.id.clone(), demand.clone());
                s.data.invocations.insert(parent.id.clone(), parent.clone());
                Ok(())
            })
            .unwrap();
        (crate::workspace_scope::ScopeController { service: &service })
            .control(
                crate::workspace_scope::ScopeAccess {
                    reservation: demand.id.clone(),
                    capability: demand.access_key.clone(),
                    namespace: "unit".into(),
                },
                crate::workspace_scope::ScopeControl::Open {
                    deadline_ms: crate::workspace_scope::now_ms() + 30000,
                    invocation_id: None,
                },
            )
            .unwrap();
        service
            .authority
            .update(|s| {
                let scope = s.data.workspace_scopes.values().next().unwrap().clone();
                let mut scope = scope;
                scope.invocation_id = Some(parent.id.clone());
                s.data.workspace_scopes.insert("scope".into(), scope);
                Ok(())
            })
            .unwrap();
        Self {
            service,
            demand,
            parent,
            child,
        }
    }
    fn capture(&self, sequence: u64, body: Value) -> Option<String> {
        let mut child = self.child.clone();
        child.id = format!("child-{sequence}");
        child.queue_order = sequence;
        let mut capture = interaction_capture::begin(&self.service, (&self.demand, &child, &body))?;
        capture.dispatched();
        let response = json!({"model":self.demand.profile.model,"choices":[{"message":{"content":"answer",
            "tool_calls":[{"id":"call-1","type":"function","function":{"name":"read","arguments":"{\"path\":\"src/lib.rs\"}"}}]},
            "finish_reason":"tool_calls"}],"usage":{"prompt_tokens":25,"completion_tokens":8}});
        capture.finish((Some(response.clone()), &Ok(response)));
        let dir = store::directory(&self.service, (&self.demand.owner, &self.demand.id));
        Some(
            store::starts(&dir, 128)
                .unwrap()
                .last()
                .unwrap()
                .artifact_id
                .clone(),
        )
    }
    fn get(&self, id: &str) -> Result<Value, String> {
        store::retrieve(&self.service, (&self.demand.owner, id))
    }
    fn close(&self, state: DemandState) {
        self.service
            .authority
            .update(|s| {
                let d = s.data.demands.get_mut(&self.demand.id).unwrap();
                d.state = state;
                d.released = true;
                if matches!(state, DemandState::Released | DemandState::Cancelled) {
                    d.reservation_closed = Some(state);
                }
                Ok(())
            })
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.service.config.authority_root);
    }
}

#[test]
fn disabled_has_no_storage_and_opt_in_is_identity_frozen() {
    let f = Fixture::new(false);
    assert!(
        f.capture(1, json!({"messages":["secret-free prompt"]}))
            .is_none()
    );
    assert!(!store::base(&f.service).exists());
    let request = f.demand.reserve_request.clone().unwrap();
    let mut enabled = request.clone();
    enabled.diagnostics.retained_model_interactions = true;
    assert_ne!(request, enabled);
    let bytes = serde_json::to_string(&request).unwrap();
    assert!(!bytes.contains("diagnostics"));
}

#[test]
fn ordered_full_requests_responses_tool_ids_usage_and_unavailable_metrics() {
    let f = Fixture::new(true);
    let body = json!({"messages":[{"role":"system","content":"system\n  indentation"},
        {"role":"user","content":"read src/lib.rs"}],"tools":[{"type":"function","function":{"name":"read"}}],
        "tool_choice":"auto","temperature":0.2,"max_tokens":128});
    let first = f.capture(1, body.clone()).unwrap();
    let second = f
        .capture(
            2,
            json!({"messages":[{"role":"tool","tool_call_id":"call-1","content":"result"}]}),
        )
        .unwrap();
    let record = f.get(&first).unwrap();
    assert_eq!(record["request"]["body"], body);
    assert_eq!(record["response"]["body"]["usage"]["completion_tokens"], 8);
    assert_eq!(
        record["response"]["body"]["choices"][0]["finish_reason"],
        "tool_calls"
    );
    assert_eq!(
        record["response"]["body"]["choices"][0]["message"]["tool_calls"][0]["id"],
        "call-1"
    );
    assert!(record["timing"]["prefill_seconds"].is_null());
    assert!(record["timing"]["first_token_seconds"].is_null());
    assert!(record["timing"]["duration_seconds"].as_f64().unwrap() >= 0.0);
    assert_eq!(f.get(&second).unwrap()["identity"]["sequence"], 2);
    let summary = store::summaries(&f.service, (&f.demand.owner, &f.parent))
        .unwrap()
        .unwrap();
    assert_eq!(summary["records"][0]["artifact_id"], first);
    assert_eq!(summary["records"][1]["artifact_id"], second);
    assert!(!first.contains(&f.demand.id));
    assert!(!first.contains('/'));
}

#[test]
fn owner_checked_redaction_preserves_source_and_whitespace() {
    let f = Fixture::new(true);
    let content = format!(
        "fn sum() {{\n    return 42;\n}}\nBearer secret-value password=secret api_key=secret\n{} /srv/private/key\nhttp://127.0.0.1:8017/v1",
        f.demand.access_key
    );
    let id = f
        .capture(
            1,
            json!({"messages":[{"role":"user","content":content}],"password":"secret"}),
        )
        .unwrap();
    assert_eq!(
        store::retrieve(&f.service, ("unrelated", &id)).unwrap_err(),
        "not_found"
    );
    let record = f.get(&id).unwrap();
    let text = record["request"]["body"]["messages"][0]["content"]
        .as_str()
        .unwrap();
    assert!(text.contains("fn sum() {\n    return 42;\n}"));
    for secret in [
        "secret-value",
        "password=secret",
        "api_key=secret",
        &f.demand.access_key,
        "/srv/private/key",
        "127.0.0.1",
    ] {
        assert!(!text.contains(secret), "{text}");
    }
    let dir = store::directory(&f.service, (&f.demand.owner, &f.demand.id));
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for entry in fs::read_dir(dir).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn all_terminal_states_hide_and_delete_traces() {
    for state in [
        DemandState::Released,
        DemandState::Cancelled,
        DemandState::Expired,
        DemandState::Preempted,
    ] {
        let f = Fixture::new(true);
        let ids = (1..=3)
            .map(|n| f.capture(n, json!({"messages":["exact prompt"]})).unwrap())
            .collect::<Vec<_>>();
        f.close(state);
        for id in ids {
            assert_eq!(f.get(&id).unwrap_err(), "not_found");
        }
        store::sweep(&f.service).unwrap();
        assert!(!store::directory(&f.service, (&f.demand.owner, &f.demand.id)).exists());
        assert!(
            store::summaries(&f.service, (&f.demand.owner, &f.parent))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn limits_preserve_metadata_and_report_omissions() {
    let mut f = Fixture::new(true);
    f.service.config.interaction_diagnostics = Limits {
        max_request_bytes: 64,
        max_response_bytes: 64,
        max_interactions_per_work: 2,
        max_interactions_per_reservation: 4,
        max_reservation_bytes: 5 * 8192,
    };
    let one = f
        .capture(1, json!({"messages":["x".repeat(10000)]}))
        .unwrap();
    let record = f.get(&one).unwrap();
    assert_eq!(record["request"]["truncated"], true);
    assert_eq!(record["response"]["truncated"], true);
    assert_eq!(record["identity"]["sequence"], 1);
    f.capture(2, json!({"messages":["two"]})).unwrap();
    assert!(f.capture(3, json!({"messages":["three"]})).is_none());
    let summary = store::summaries(&f.service, (&f.demand.owner, &f.parent))
        .unwrap()
        .unwrap();
    assert_eq!(summary["records"].as_array().unwrap().len(), 2);
    assert_eq!(summary["omitted_interactions"], 1);
}

#[test]
fn reservation_content_budget_and_json_escaping_are_bounded() {
    let mut f = Fixture::new(true);
    f.service.config.interaction_diagnostics = Limits {
        max_request_bytes: 65536,
        max_response_bytes: 65536,
        max_interactions_per_work: 4,
        max_interactions_per_reservation: 4,
        max_reservation_bytes: 5 * 8192,
    };
    let id = f
        .capture(1, json!({"messages":["\\\"".repeat(1000)]}))
        .unwrap();
    let record = f.get(&id).unwrap();
    assert_eq!(
        record["request"]["unavailable_reason"],
        "reservation_content_limit"
    );
    assert_eq!(record["identity"]["sequence"], 1);
    let value = store::Content::bounded(json!("\n".repeat(5000)), 64).unwrap();
    assert!(serde_json::to_vec(&value).unwrap().len() <= 64 + 512);
}

#[test]
fn restart_preserves_active_records_and_sequence_without_replay() {
    let f = Fixture::new(true);
    let first = f.capture(7, json!({"messages":["one"]})).unwrap();
    let restarted = Service::new(f.service.config.clone());
    restarted.recover().unwrap();
    assert!(store::retrieve(&restarted, (&f.demand.owner, &first)).is_ok());
    let second = f.capture(8, json!({"messages":["two"]})).unwrap();
    assert_ne!(first, second);
    assert_eq!(f.get(&second).unwrap()["identity"]["sequence"], 8);
    f.close(DemandState::Released);
    restarted.recover().unwrap();
    assert_eq!(
        store::retrieve(&restarted, (&f.demand.owner, &first)).unwrap_err(),
        "not_found"
    );
    assert!(!store::directory(&restarted, (&f.demand.owner, &f.demand.id)).exists());
}

#[test]
fn deletion_failure_is_bounded_obligation_retried_after_restart() {
    let f = Fixture::new(true);
    let id = f.capture(1, json!({"messages":["one"]})).unwrap();
    let dir = store::directory(&f.service, (&f.demand.owner, &f.demand.id));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    f.close(DemandState::Released);
    assert!(store::sweep(&f.service).is_err());
    assert_eq!(f.get(&id).unwrap_err(), "not_found");
    assert!(dir.exists());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let restarted = Service::new(f.service.config.clone());
    restarted.recover().unwrap();
    assert!(!dir.exists());
}

#[test]
fn capture_write_failure_and_late_completion_never_resurrect() {
    let f = Fixture::new(true);
    fs::create_dir_all(&f.service.config.authority_root).unwrap();
    fs::write(store::base(&f.service), "not a directory").unwrap();
    assert!(f.capture(1, json!({"messages":["one"]})).is_none());
    fs::remove_file(store::base(&f.service)).unwrap();
    let mut capture = interaction_capture::begin(
        &f.service,
        (&f.demand, &f.child, &json!({"messages":["one"]})),
    )
    .unwrap();
    capture.dispatched();
    f.close(DemandState::Released);
    store::sweep(&f.service).unwrap();
    capture.finish((
        Some(json!({"content":"late"})),
        &Ok(json!({"content":"late"})),
    ));
    assert!(!store::directory(&f.service, (&f.demand.owner, &f.demand.id)).exists());
}

#[test]
fn partial_record_reports_missing_completion_and_backend_error_is_retained() {
    let f = Fixture::new(true);
    let capture = interaction_capture::begin(
        &f.service,
        (&f.demand, &f.child, &json!({"messages":["one"]})),
    )
    .unwrap();
    let dir = store::directory(&f.service, (&f.demand.owner, &f.demand.id));
    let id = store::starts(&dir, 128).unwrap()[0].artifact_id.clone();
    let partial = f.get(&id).unwrap();
    assert_eq!(partial["diagnostic_complete"], false);
    assert!(partial["response"].is_null());
    capture.finish((
        Some(json!({"error":{"message":"backend rejected"}})),
        &Err("backend_http_failure".into()),
    ));
    let completed = f.get(&id).unwrap();
    assert_eq!(completed["error"], "backend_http_failure");
    assert_eq!(
        completed["response"]["body"]["error"]["message"],
        "backend rejected"
    );
}

#[test]
fn group_diagnostics_remain_until_the_last_member_closes() {
    let f = Fixture::new(true);
    let id = f.capture(1, json!({"messages":["one"]})).unwrap();
    f.service
        .authority
        .update(|s| {
            let mut peer = f.demand.clone();
            peer.id = "peer".into();
            peer.reservation_id = Some(f.demand.id.clone());
            peer.reserve_request = None;
            peer.state = DemandState::Ready;
            let root = s.data.demands.get_mut(&f.demand.id).unwrap();
            root.services
                .insert(root.profile.tag.clone(), root.id.clone());
            root.services.insert("peer-service".into(), peer.id.clone());
            root.state = DemandState::Preempted;
            s.data.demands.insert(peer.id.clone(), peer);
            Ok(())
        })
        .unwrap();
    store::sweep(&f.service).unwrap();
    assert!(f.get(&id).is_ok());
    f.service
        .authority
        .update(|s| {
            s.data.demands.get_mut("peer").unwrap().state = DemandState::Expired;
            Ok(())
        })
        .unwrap();
    assert_eq!(f.get(&id).unwrap_err(), "not_found");
    store::sweep(&f.service).unwrap();
}

#[test]
fn concurrent_capture_cannot_overspend_and_sweep_cursor_is_fair() {
    let mut f = Fixture::new(true);
    f.service.config.interaction_diagnostics = Limits {
        max_request_bytes: 512,
        max_response_bytes: 512,
        max_interactions_per_work: 8,
        max_interactions_per_reservation: 8,
        max_reservation_bytes: 9 * 8192 + 2048,
    };
    std::thread::scope(|scope| {
        for sequence in 1..=16 {
            let f = &f;
            scope.spawn(move || {
                f.capture(sequence, json!({"messages":["x".repeat(1024)]}));
            });
        }
    });
    let dir = store::directory(&f.service, (&f.demand.owner, &f.demand.id));
    let calls = store::starts(&dir, 8).unwrap();
    assert_eq!(calls.len(), 8);
    assert!(calls.iter().map(|c| c.charged_content_bytes).sum::<u64>() <= 2048);
    let bytes = fs::read_dir(&dir)
        .unwrap()
        .map(|p| p.unwrap().metadata().unwrap().len())
        .sum::<u64>();
    assert!(
        bytes
            <= f.service
                .config
                .interaction_diagnostics
                .max_reservation_bytes
    );
    for n in 0..70 {
        let dir = store::directory(&f.service, ("orphan", &format!("closed-{n}")));
        store::private_directory(&dir).unwrap();
        store::save_lease(
            &dir,
            &store::Lease {
                owner: "orphan".into(),
                reservation_id: format!("closed-{n}"),
                limits: Limits::default(),
                omitted_interactions: 0,
                cleanup_pending: true,
            },
        )
        .unwrap();
    }
    for _ in 0..4 {
        store::sweep(&f.service).unwrap();
    }
    assert_eq!(
        fs::read_dir(store::base(&f.service))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_dir())
            .count(),
        1
    );
}
