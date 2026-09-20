use crate::{
    admission::Admission,
    config::{Config, Driver},
    control::{Action, Control, ControlContext, ReservationControl},
    idle,
    inference::Submission,
    service::Service,
    types::*,
};
use std::{fs, sync::Arc};
struct Fixture {
    service: Service,
}
impl Fixture {
    fn new() -> Self {
        let mut config: Config =
            serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                .unwrap();
        config.authority_root =
            std::env::temp_dir().join(format!("rack-idle-{}", identity().unwrap()));
        fs::create_dir_all(&config.authority_root).unwrap();
        config.fixture_mode = true;
        for p in &mut config.profiles {
            p.driver = Driver::Fixture;
            p.qualified = true;
            p.evidence = vec!["synthetic-no-gpu".into()];
        }
        Self {
            service: Service::new(config),
        }
    }
    fn acquire(&self, args: (&str, &str, Priority)) -> Demand {
        let (owner, tag, priority) = args;
        let source = self
            .service
            .config
            .sources
            .iter()
            .find(|s| s.source == owner)
            .unwrap();
        let profile = self
            .service
            .config
            .profiles
            .iter()
            .find(|p| p.tag == tag)
            .unwrap();
        let request = Acquire {
            schema: VERSION.into(),
            source_system: owner.into(),
            work_id: "work".into(),
            acquisition_id: identity().unwrap(),
            tag: tag.into(),
            priority: Some(priority),
            capabilities: profile.capabilities.clone(),
            context_tokens: 1024,
            ttl_seconds: 86400,
            qualification: false,
        };
        Admission {
            service: &self.service,
            source,
        }
        .acquire(request)
        .unwrap()
    }
    fn ready(&self, d: &Demand) {
        self.service
            .authority
            .update(|s| {
                let demand = s.data.demands.get_mut(&d.id).unwrap();
                demand.state = DemandState::Ready;
                demand.ready_checked = true;
                for resource in demand.profile.resources.clone() {
                    s.claims.insert(resource, demand.id.clone());
                }
                Ok(())
            })
            .unwrap();
    }
    fn inspect(&self, d: &Demand) -> Demand {
        self.service.inspect(&d.owner, &d.id).unwrap()
    }
    fn reap(&self, at: u64) {
        self.service
            .authority
            .update(|s| {
                idle::reap(s, 1800, at);
                Ok(())
            })
            .unwrap();
    }
    fn retire(&self, d: &Demand) {
        crate::retirement::Retirement {
            service: &self.service,
        }
        .run(&self.inspect(d))
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.service.config.authority_root).unwrap();
    }
}
fn request(d: &Demand) -> Inference {
    Inference {
        work: None,
        schema: VERSION.into(),
        submission_id: identity().unwrap(),
        reservation_id: d.id.clone(),
        generation: d.generation.clone(),
        profile_hash: d.profile_hash.clone(),
        prompt: "hello".into(),
        payload: None,
        max_tokens: 16,
        timeout_seconds: 5,
        wait_seconds: None,
        workspace_scope: None,
    }
}
#[test]
fn independent_cb_paramount_activity_and_exact_idle_threshold() {
    let f = Fixture::new();
    let primary = f.acquire(("cb", "local-primary", Priority::Paramount));
    let media = f.acquire(("cb", "local-image", Priority::Paramount));
    assert_eq!(primary.state, DemandState::Preparing);
    assert_eq!(media.state, DemandState::Preparing);
    assert_ne!(primary.profile.resources, media.profile.resources);
    f.ready(&primary);
    f.ready(&media);
    let origin = primary.created;
    f.service
        .authority
        .update(|s| idle::touch(s, &media.id, origin + 600))
        .unwrap();
    f.reap(origin + 1799);
    assert!(!f.inspect(&primary).released);
    f.reap(origin + 1800);
    assert_eq!(f.inspect(&primary).reason.as_deref(), Some("idle_timeout"));
    assert!(!f.inspect(&media).released);
    f.retire(&primary);
    assert_eq!(f.inspect(&primary).state, DemandState::Expired);
    f.reap(origin + 2399);
    assert!(!f.inspect(&media).released);
    f.reap(origin + 2400);
    assert!(f.inspect(&media).released);
}
#[test]
fn polling_renewal_replay_and_wrong_owner_do_not_refresh_activity() {
    let f = Fixture::new();
    let d = f.acquire(("cb", "local-primary", Priority::Paramount));
    f.ready(&d);
    let before = fs::read(f.service.config.authority_root.join("managed.json")).unwrap();
    for _ in 0..5 {
        assert_eq!(f.inspect(&d).last_activity_at, None);
    }
    assert_eq!(
        before,
        fs::read(f.service.config.authority_root.join("managed.json")).unwrap()
    );
    assert!(
        Submission {
            service: &f.service
        }
        .submit("athba", request(&d))
        .is_err()
    );
    ReservationControl {
        service: &f.service,
    }
    .control(ControlContext {
        owner: &d.owner,
        id: &d.id,
        request: Control {
            generation: d.generation.clone(),
            action: Action::Renew { ttl_seconds: 86400 },
        },
    })
    .unwrap();
    assert_eq!(f.inspect(&d).last_activity_at, None);
    f.reap(d.created + 1800);
    assert!(f.inspect(&d).released);
    assert!(
        ReservationControl {
            service: &f.service
        }
        .control(ControlContext {
            owner: &d.owner,
            id: &d.id,
            request: Control {
                generation: d.generation.clone(),
                action: Action::Renew { ttl_seconds: 86400 }
            }
        })
        .is_err()
    );
}
#[test]
fn admitted_inference_updates_only_its_resource_and_replay_is_read_only() {
    let f = Fixture::new();
    let d = f.acquire(("cb", "local-primary", Priority::Paramount));
    f.ready(&d);
    let media = f.acquire(("cb", "local-image", Priority::Paramount));
    f.ready(&media);
    let req = request(&d);
    let i = Submission {
        service: &f.service,
    }
    .submit("cb", req.clone())
    .unwrap();
    assert!(f.inspect(&d).last_activity_at.is_some());
    assert!(f.inspect(&media).last_activity_at.is_none());
    let before = fs::read(f.service.config.authority_root.join("managed.json")).unwrap();
    assert_eq!(
        Submission {
            service: &f.service
        }
        .submit("cb", req)
        .unwrap()
        .id,
        i.id
    );
    f.service.result("cb", &i.id).unwrap();
    assert_eq!(
        before,
        fs::read(f.service.config.authority_root.join("managed.json")).unwrap()
    );
}
#[test]
fn started_and_uncertain_work_keep_claims_and_restart_evidence() {
    for state in [InvocationState::Running, InvocationState::Uncertain] {
        let f = Fixture::new();
        let d = f.acquire(("cb", "local-primary", Priority::Paramount));
        f.ready(&d);
        let i = Submission {
            service: &f.service,
        }
        .submit("cb", request(&d))
        .unwrap();
        f.service
            .authority
            .update(|s| {
                s.data.invocations.get_mut(&i.id).unwrap().state = state;
                Ok(())
            })
            .unwrap();
        f.reap(d.created + 3600);
        assert!(!f.inspect(&d).released);
        f.service.recover().unwrap();
        assert_eq!(
            f.service.result("cb", &i.id).unwrap().state,
            InvocationState::Uncertain
        );
        f.reap(d.created + 7200);
        assert!(!f.inspect(&d).released);
    }
}
#[test]
fn idle_release_reacquisition_and_athba_priority_are_normal_admission() {
    let f = Fixture::new();
    let cb = f.acquire(("cb", "local-primary", Priority::Paramount));
    f.ready(&cb);
    assert_eq!(
        f.acquire(("athba", "local-primary", Priority::Medium))
            .state,
        DemandState::Unavailable
    );
    let source = f
        .service
        .config
        .sources
        .iter()
        .find(|s| s.source == "athba")
        .unwrap();
    let mut paramount = cb.request.clone();
    paramount.source_system = "athba".into();
    let admitted = Admission {
        service: &f.service,
        source,
    }
    .acquire(paramount)
    .unwrap();
    assert_eq!(admitted.priority, Priority::Paramount);
    assert_eq!(admitted.state, DemandState::Unavailable);
    f.reap(cb.created + 1800);
    f.retire(&cb);
    let athba = f.acquire(("athba", "local-primary", Priority::Medium));
    f.ready(&athba);
    let fresh = f.acquire(("cb", "local-primary", Priority::Paramount));
    assert_eq!(fresh.state, DemandState::Preparing);
    assert_eq!(fresh.victims, vec![athba.id]);
    assert_ne!(fresh.id, cb.id);
    let tie = f.acquire(("cb", "local-primary", Priority::Paramount));
    assert_eq!(tie.state, DemandState::Unavailable);
}
#[test]
fn explicit_release_works_and_does_not_erase_idle_reason() {
    for idle_first in [false, true] {
        let f = Fixture::new();
        let d = f.acquire(("cb", "local-primary", Priority::Paramount));
        f.ready(&d);
        if idle_first {
            f.reap(d.created + 1800);
        }
        ReservationControl {
            service: &f.service,
        }
        .control(ControlContext {
            owner: "cb",
            id: &d.id,
            request: Control {
                generation: d.generation.clone(),
                action: Action::Release,
            },
        })
        .unwrap();
        f.retire(&d);
        assert_eq!(
            f.inspect(&d).state,
            if idle_first {
                DemandState::Expired
            } else {
                DemandState::Released
            }
        );
        f.service
            .authority
            .read(|s| {
                assert!(s.claims.is_empty());
                Ok(())
            })
            .unwrap();
    }
}
#[test]
fn concurrent_submission_and_idle_reaper_have_one_authoritative_winner() {
    for _ in 0..20 {
        let f = Arc::new(Fixture::new());
        let d = f.acquire(("cb", "local-primary", Priority::Paramount));
        f.ready(&d);
        let at = now();
        f.service
            .authority
            .update(|s| {
                s.data.demands.get_mut(&d.id).unwrap().created = at - 1800;
                Ok(())
            })
            .unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let other = Arc::clone(&f);
        let start = Arc::clone(&barrier);
        let reaper = std::thread::spawn(move || {
            start.wait();
            other.reap(at);
        });
        barrier.wait();
        let result = Submission {
            service: &f.service,
        }
        .submit("cb", request(&d));
        reaper.join().unwrap();
        let current = f.inspect(&d);
        if result.is_ok() {
            assert!(!current.released);
            assert!(current.last_activity_at.unwrap() >= at);
        } else {
            assert!(current.released);
            assert_eq!(current.reason.as_deref(), Some("idle_timeout"));
        }
    }
}
#[test]
fn network_profiles_require_one_matching_loopback_binding() {
    let f = Fixture::new();
    let mut p = f.service.config.profiles[0].clone();
    for args in [
        vec![],
        vec!["--host", "0.0.0.0"],
        vec!["--host", "::"],
        vec!["--host", "127.0.0.1", "--host=0.0.0.0"],
        vec!["--host", "::1"],
    ] {
        p.args = args.into_iter().map(String::from).collect();
        assert!(crate::network::private_binding(&p).is_err());
    }
    for args in [vec!["--host", "127.0.0.1"], vec!["--host=127.0.0.1"]] {
        p.args = args.into_iter().map(String::from).collect();
        crate::network::private_binding(&p).unwrap();
    }
}
#[test]
fn legacy_missing_activity_is_not_refreshed_by_read_or_recovery() {
    let f = Fixture::new();
    let d = f.acquire(("cb", "local-primary", Priority::Paramount));
    f.ready(&d);
    let path = f.service.config.authority_root.join("managed.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["data"]["demands"][&d.id]
        .as_object_mut()
        .unwrap()
        .remove("last_activity_at");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    f.service.recover().unwrap();
    assert!(f.inspect(&d).last_activity_at.is_none());
    f.reap(d.created + 1800);
    assert!(f.inspect(&d).released);
}

#[test]
fn historical_profile_without_max_input_tokens_recovers_but_config_stays_strict() {
    let f = Fixture::new();
    let d = f.acquire(("cb", "local-primary", Priority::Low));
    let path = f.service.config.authority_root.join("managed.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["data"]["demands"][&d.id]["profile"]
        .as_object_mut()
        .unwrap()
        .remove("max_input_tokens");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

    f.service.recover().unwrap();
    let historical = f.inspect(&d);
    assert_eq!(historical.profile.max_input_tokens, 0);
    assert_eq!(
        historical.profile.effective_max_input_tokens(),
        historical
            .profile
            .context_tokens
            .saturating_sub(historical.profile.max_output_tokens)
    );
    assert_eq!(
        crate::api::public(historical.clone()).unwrap()["max_input_tokens"],
        historical.profile.effective_max_input_tokens()
    );

    let mut config_value: serde_json::Value =
        serde_json::from_str(include_str!("../../../config/runtime/config.example.json")).unwrap();
    config_value["profiles"][0]
        .as_object_mut()
        .unwrap()
        .remove("max_input_tokens");
    let config: Config = serde_json::from_value(config_value).unwrap();
    assert_eq!(config.profiles[0].max_input_tokens, 0);
    assert_eq!(
        crate::validation::validate(&config).unwrap_err(),
        format!("invalid runtime profile: {}", config.profiles[0].tag)
    );
}

#[test]
fn idle_policy_defaults_and_invalid_values_fail_validation() {
    let original: serde_json::Value =
        serde_json::from_str(include_str!("../../../config/runtime/config.example.json")).unwrap();
    let mut legacy = original.clone();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("idle_timeout_seconds");
    let config: Config = serde_json::from_value(legacy).unwrap();
    assert_eq!(config.idle_timeout_seconds, 1800);
    crate::validation::validate(&config).unwrap();
    for seconds in [0, 86401] {
        let mut invalid = original.clone();
        invalid["idle_timeout_seconds"] = serde_json::json!(seconds);
        let config: Config = serde_json::from_value(invalid).unwrap();
        assert!(crate::validation::validate(&config).is_err());
    }
}

#[test]
fn historical_chatterbox_record_reads_unchanged() {
    let f = Fixture::new();
    let d = f.acquire(("cb", "local-primary", Priority::Low));
    let path = f.service.config.authority_root.join("managed.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["data"]["demands"][&d.id]["profile"]["backend"] = "chatterbox".into();
    let original = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &original).unwrap();
    let historical = f.inspect(&d);
    assert_eq!(
        historical.profile.backend,
        crate::config::Backend::Chatterbox
    );
    assert_eq!(
        serde_json::to_value(&historical.profile).unwrap()["backend"],
        "chatterbox"
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let mut config = f.service.config.clone();
    crate::validation::validate(&config).unwrap();
    config.profiles[0].backend = historical.profile.backend;
    assert_eq!(
        crate::validation::validate(&config).unwrap_err(),
        "invalid_chatterbox_profile"
    );
}

#[test]
fn historical_speech_record_reads_unchanged_and_invalid_payload_is_rejected() {
    let f = Fixture::new();
    let d = f.acquire(("cb", "local-primary", Priority::Low));
    f.ready(&d);
    let i = Submission {
        service: &f.service,
    }
    .submit("cb", request(&d))
    .unwrap();
    let path = f.service.config.authority_root.join("managed.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["data"]["demands"][&d.id]["profile"]["protocols"] = serde_json::json!(["speech"]);
    value["data"]["invocations"][&i.id]["request"]["payload"] = serde_json::json!({
        "protocol": "speech", "body": {"model": d.profile.model, "input": "historical text"}
    });
    let original = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &original).unwrap();
    let historical = f.inspect(&d);
    let invocation = f.service.result("cb", &i.id).unwrap();
    let payload = invocation.request.payload.as_ref().unwrap();
    assert_eq!(payload.protocol, crate::protocol::Protocol::Speech);
    assert_eq!(
        historical.profile.protocols,
        vec![crate::protocol::Protocol::Speech]
    );
    assert_eq!(serde_json::to_value(payload).unwrap()["protocol"], "speech");
    assert_eq!(
        serde_json::to_value(&historical.profile).unwrap()["protocols"],
        serde_json::json!(["speech"])
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let mut config = f.service.config.clone();
    crate::validation::validate(&config).unwrap();
    config.profiles[0].protocols = historical.profile.protocols.clone();
    assert_eq!(
        crate::validation::validate(&config).unwrap_err(),
        "speech_backend_required"
    );
    assert_eq!(
        payload.validate(&historical).unwrap_err(),
        "invalid_speech_request"
    );
    assert_eq!(
        crate::protocol::raw_result("{}".into(), payload).unwrap_err(),
        "use_speech_interface"
    );
    assert_eq!(fs::read(&path).unwrap(), original);
}

fn grouped_models(f: &Fixture) -> Vec<Demand> {
    let source = f
        .service
        .config
        .sources
        .iter()
        .find(|s| s.source == "cb")
        .unwrap();
    let result = (crate::reservation_admission::ReservationAdmission {
        service: &f.service,
        source,
    })
    .reserve(crate::reservation::Reserve {
        acquisition_id: identity().unwrap(),
        work_id: "shared-idle".into(),
        services: vec!["local-primary".into(), "local-coder".into()],
        priority: Priority::Low,
        ttl_seconds: 86400,
    })
    .unwrap();
    let parent = f
        .service
        .inspect("cb", result["id"].as_str().unwrap())
        .unwrap();
    let mut members: Vec<_> = parent
        .services
        .values()
        .map(|id| f.service.inspect("cb", id).unwrap())
        .collect();
    members.sort_by_key(|d| d.profile.tag.clone());
    for d in &members {
        f.ready(d);
    }
    members
}

#[test]
fn grouped_model_activity_retains_idle_peer_until_shared_threshold() {
    for active_member in 0..2 {
        let f = Fixture::new();
        let members = grouped_models(&f);
        let origin = members.iter().map(|d| d.created).max().unwrap();
        f.service
            .authority
            .update(|s| idle::touch(s, &members[active_member].id, origin + 1200))
            .unwrap();
        f.reap(origin + 1800);
        assert!(members.iter().all(|d| !f.inspect(d).released));
        f.reap(origin + 2999);
        assert!(members.iter().all(|d| !f.inspect(d).released));
        f.reap(origin + 3000);
        assert!(
            members
                .iter()
                .all(|d| f.inspect(d).reason.as_deref() == Some("idle_timeout"))
        );
    }
}

#[test]
fn grouped_inflight_inference_protects_all_members_without_extending_deadlines() {
    for state in [InvocationState::Running, InvocationState::Uncertain] {
        let f = Fixture::new();
        let members = grouped_models(&f);
        let invocation = (Submission {
            service: &f.service,
        })
        .submit("cb", request(&members[0]))
        .unwrap();
        f.service
            .authority
            .update(|s| {
                s.data.invocations.get_mut(&invocation.id).unwrap().state = state;
                Ok(())
            })
            .unwrap();
        f.reap(members[0].created + 3600);
        for d in &members {
            assert!(!f.inspect(d).released);
            assert_eq!(f.inspect(d).deadline, d.deadline);
        }
    }
}

#[test]
fn grouped_recent_activity_does_not_override_explicit_release() {
    let f = Fixture::new();
    let members = grouped_models(&f);
    f.service
        .authority
        .update(|s| idle::touch(s, &members[0].id, now()))
        .unwrap();
    let parent = members[0].reservation_id.as_ref().unwrap();
    crate::control::reservation_control(&f.service, ("cb", parent, Action::Release)).unwrap();
    assert!(members.iter().all(|d| f.inspect(d).released));
}

#[test]
fn activity_retention_extends_group_not_other_reservations() {
    let f = Fixture::new();
    let members = grouped_models(&f);
    let at = now();
    let unrelated = f.acquire(("athba", "comfyui", Priority::Low));
    f.ready(&unrelated);
    f.service
        .authority
        .update(|s| {
            for d in &members {
                s.data.demands.get_mut(&d.id).unwrap().deadline = at + 15;
            }
            idle::touch(s, &members[0].id, at + 10)?;
            crate::activity_retention::refresh(s, (30, at + 10))?;
            for d in &members {
                assert_eq!(s.data.demands[&d.id].deadline, at + 40);
            }
            assert_eq!(s.data.demands[&unrelated.id].deadline, unrelated.deadline);
            Ok(())
        })
        .unwrap();
}

#[test]
fn activity_retention_protects_long_running_and_uncertain_work() {
    for state in [InvocationState::Running, InvocationState::Uncertain] {
        let f = Fixture::new();
        let members = grouped_models(&f);
        let invocation = (Submission {
            service: &f.service,
        })
        .submit("cb", request(&members[0]))
        .unwrap();
        let at = now();
        f.service
            .authority
            .update(|s| {
                for d in &members {
                    s.data.demands.get_mut(&d.id).unwrap().deadline = at + 5;
                }
                s.data.invocations.get_mut(&invocation.id).unwrap().state = state;
                for elapsed in [1, 4, 7, 10] {
                    crate::activity_retention::refresh(s, (5, at + elapsed))?;
                    for d in &members {
                        assert_eq!(s.data.demands[&d.id].deadline, at + elapsed + 5);
                    }
                }
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn activity_retention_does_not_revive_terminal_unowned_or_elapsed_members() {
    for state in [
        DemandState::Released,
        DemandState::Cancelled,
        DemandState::Expired,
        DemandState::Preempted,
        DemandState::RecoveryRequired,
        DemandState::Preparing,
    ] {
        let f = Fixture::new();
        let members = grouped_models(&f);
        let at = now();
        f.service
            .authority
            .update(|s| {
                for d in &members {
                    s.data.demands.get_mut(&d.id).unwrap().deadline = at + 5;
                }
                idle::touch(s, &members[0].id, at)?;
                s.data.demands.get_mut(&members[1].id).unwrap().state = state;
                crate::activity_retention::refresh(s, (30, at))?;
                assert_eq!(s.data.demands[&members[1].id].deadline, at + 5);
                s.data.demands.get_mut(&members[1].id).unwrap().state = DemandState::Ready;
                crate::activity_retention::refresh(s, (30, at + 5))?;
                assert_eq!(s.data.demands[&members[1].id].deadline, at + 5);
                s.data.demands.get_mut(&members[1].id).unwrap().deadline = at + 10;
                s.claims.retain(|_, id| id != &members[1].id);
                crate::activity_retention::refresh(s, (30, at + 5))?;
                assert_eq!(s.data.demands[&members[1].id].deadline, at + 10);
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn activity_retention_requires_activity_and_respects_closed_group() {
    let f = Fixture::new();
    let members = grouped_models(&f);
    let at = now();
    f.service
        .authority
        .update(|s| {
            for d in &members {
                s.data.demands.get_mut(&d.id).unwrap().deadline = at + 5;
            }
            crate::activity_retention::refresh(s, (30, at))?;
            for d in &members {
                assert_eq!(s.data.demands[&d.id].deadline, at + 5);
            }
            idle::touch(s, &members[0].id, at)?;
            let root = members[0].reservation_id.as_ref().unwrap();
            s.data.demands.get_mut(root).unwrap().reservation_closed = Some(DemandState::Released);
            crate::activity_retention::refresh(s, (30, at))?;
            for d in &members {
                assert_eq!(s.data.demands[&d.id].deadline, at + 5);
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn activity_retention_does_not_renew_old_activity() {
    let f = Fixture::new();
    let members = grouped_models(&f);
    let at = now();
    f.service
        .authority
        .update(|s| {
            for d in &members {
                s.data.demands.get_mut(&d.id).unwrap().deadline = at + 50;
            }
            idle::touch(s, &members[0].id, at)?;
            crate::activity_retention::refresh(s, (30, at + 40))?;
            for d in &members {
                assert_eq!(s.data.demands[&d.id].deadline, at + 50);
            }
            assert!(!crate::activity_retention::pending(s, (30, at + 40)));
            Ok(())
        })
        .unwrap();
}
