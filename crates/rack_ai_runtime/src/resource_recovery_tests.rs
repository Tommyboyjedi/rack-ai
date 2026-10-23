use crate::{
    admission::Admission,
    config::{Backend, Config, Driver},
    resource_recovery::{self, ResourceStatus::*},
    service::Service,
    types::*,
};
struct Fixture {
    service: Service,
}
impl Fixture {
    fn new() -> Self {
        let mut config: Config =
            serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                .unwrap();
        config.authority_root =
            std::env::temp_dir().join(format!("rack-resource-recovery-{}", identity().unwrap()));
        std::fs::create_dir_all(&config.authority_root).unwrap();
        config.profiles.retain(|p| p.backend != Backend::Comfyui);
        config.fixture_mode = true;
        for p in &mut config.profiles {
            p.driver = Driver::Fixture;
            p.qualified = true;
        }
        Self {
            service: Service::new(config),
        }
    }
    fn demand(&self) -> Demand {
        let p = &self.service.config.profiles[0];
        let source = &self.service.config.sources[0];
        Admission {
            service: &self.service,
            source,
        }
        .acquire(Acquire {
            schema: VERSION.into(),
            source_system: source.source.clone(),
            work_id: "fixture".into(),
            acquisition_id: identity().unwrap(),
            tag: p.tag.clone(),
            priority: Some(Priority::Low),
            capabilities: p.capabilities.clone(),
            context_tokens: 1024,
            ttl_seconds: 3600,
            qualification: false,
        })
        .unwrap()
    }
    fn status(&self, id: &str, apply: bool) -> resource_recovery::ResourceStatus {
        resource_recovery::run(&self.service, apply)
            .unwrap()
            .resources
            .into_iter()
            .find(|r| r.resource == id)
            .unwrap()
            .status
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.service.config.authority_root).unwrap();
    }
}
#[test]
fn active_owner_and_running_work_are_not_reset() {
    let f = Fixture::new();
    let d = f.demand();
    let resource = d.profile.resources[0].clone();
    f.service
        .authority
        .update(|s| {
            let d = s.data.demands.get_mut(&d.id).unwrap();
            d.state = DemandState::Ready;
            Ok(())
        })
        .unwrap();
    let before = std::fs::read(f.service.config.authority_root.join("managed.json")).unwrap();
    let report = resource_recovery::run(&f.service, true).unwrap();
    let item = report
        .resources
        .iter()
        .find(|r| r.resource == resource)
        .unwrap();
    assert_eq!(item.status, InUsePreserved);
    assert!(
        serde_json::to_value(item)
            .unwrap()
            .get("preserved_service")
            .is_none()
    );
    assert_eq!(
        std::fs::read(f.service.config.authority_root.join("managed.json")).unwrap(),
        before
    );
}
#[test]
fn only_closed_recovery_is_retried_and_cancel_intent_is_preserved() {
    let f = Fixture::new();
    let d = f.demand();
    let resource = d.profile.resources[0].clone();
    f.service
        .authority
        .update(|s| {
            let d = s.data.demands.get_mut(&d.id).unwrap();
            d.state = DemandState::RecoveryRequired;
            d.released = true;
            d.reservation_closed = Some(DemandState::Cancelled);
            d.recovery_error = Some("previous cleanup failed".into());
            d.transition_deadline = now() + 1000;
            Ok(())
        })
        .unwrap();
    assert_eq!(f.status(&resource, false), CleanupPending);
    assert!(
        f.service
            .inspect(&d.owner, &d.id)
            .unwrap()
            .recovery_error
            .is_some()
    );
    assert_eq!(f.status(&resource, true), CleanupRequested);
    let after = f.service.inspect(&d.owner, &d.id).unwrap();
    assert_eq!(after.state, DemandState::RecoveryRequired);
    assert_eq!(after.reservation_closed, Some(DemandState::Cancelled));
    assert!(after.recovery_error.is_none());
    assert!(after.transition_deadline <= now());
    assert!(
        f.service
            .authority
            .read(|s| Ok(s.claims.get(&resource) == Some(&d.id)))
            .unwrap()
    );
}
#[test]
fn open_recovery_and_unknown_ownership_are_preserved() {
    let f = Fixture::new();
    let d = f.demand();
    let resource = d.profile.resources[0].clone();
    f.service
        .authority
        .update(|s| {
            s.data.demands.get_mut(&d.id).unwrap().state = DemandState::RecoveryRequired;
            Ok(())
        })
        .unwrap();
    assert_eq!(f.status(&resource, true), UnresolvedWorkPreserved);
    f.service
        .authority
        .update(|s| {
            s.claims.insert(resource.clone(), "unproven".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(f.status(&resource, true), BlockedUnprovenOwner);
}
#[test]
fn historical_terminal_records_are_never_revived() {
    let f = Fixture::new();
    let d = f.demand();
    let resource = d.profile.resources[0].clone();
    f.service
        .authority
        .update(|s| {
            s.claims.remove(&resource);
            let d = s.data.demands.get_mut(&d.id).unwrap();
            d.state = DemandState::Released;
            d.released = true;
            Ok(())
        })
        .unwrap();
    assert_eq!(f.status(&resource, true), Available);
    assert_eq!(
        f.service.inspect(&d.owner, &d.id).unwrap().state,
        DemandState::Released
    );
}
#[test]
fn healthy_warm_model_is_preserved_and_failed_eviction_uses_existing_cleanup() {
    let f = Fixture::new();
    let d = f.demand();
    let resource = d.profile.resources[0].clone();
    let record = WarmResidency {
        id: "warm-fixture".into(),
        profile_hash: d.profile_hash.clone(),
        profile: d.profile.clone(),
        backend: d.profile.backend.clone(),
        driver: d.profile.driver.clone(),
        model: d.profile.model.clone(),
        resources: d.profile.resources.clone(),
        endpoint: d.profile.endpoint.clone(),
        process: Process {
            pid: 1,
            boot: "fixture".into(),
            start: "fixture".into(),
            activation: "fixture".into(),
            unit: None,
            container: None,
            invocation: None,
        },
        cached_at: now(),
        last_ready_at: now(),
        state: WarmResidencyState::Resident,
    };
    f.service
        .authority
        .update(|s| {
            s.claims.remove(&resource);
            s.data
                .warm_residencies
                .insert(record.id.clone(), record.clone());
            Ok(())
        })
        .unwrap();
    for apply in [false, true] {
        let report = resource_recovery::run(&f.service, apply).unwrap();
        let item = report
            .resources
            .iter()
            .find(|r| r.resource == resource)
            .unwrap();
        assert_eq!(item.status, WarmPreserved);
        assert_eq!(
            item.preserved_service.as_deref(),
            Some(record.profile.tag.as_str())
        );
    }
    f.service
        .authority
        .update(|s| {
            s.data.warm_residencies.get_mut(&record.id).unwrap().state =
                WarmResidencyState::RecoveryRequired;
            Ok(())
        })
        .unwrap();
    assert_eq!(f.status(&resource, true), CleanupRequested);
    assert!(
        f.service
            .authority
            .read(|s| Ok(s.data.warm_residencies[&record.id].state == WarmResidencyState::Evicting))
            .unwrap()
    );
}
