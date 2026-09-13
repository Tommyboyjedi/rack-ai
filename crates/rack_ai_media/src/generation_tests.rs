use crate::{backend_generation::BackendGeneration, systemd::Observation, types::Service};
#[test]
fn generation_verifies_pid_invocation_and_kernel_start_identity() {
    let mut observed = Observation {
        invocation: "first".into(),
        pid: std::process::id(),
        cgroup: "/fixture-owned".into(),
        active: "active".into(),
        pending_job: false,
    };
    let generation = BackendGeneration::capture(1, &observed).unwrap();
    generation.verify(&observed).unwrap();
    observed.invocation = "replacement".into();
    assert!(generation.verify(&observed).is_err());
    observed.invocation = "first".into();
    let mut reused = generation.clone();
    reused.start_ticks += 1;
    assert!(reused.verify(&observed).is_err());
    observed.pid = 0;
    assert!(generation.verify(&observed).is_err());
}
#[test]
fn old_service_state_loads_without_inventing_restart_authorization() {
    let mut old = serde_json::to_value(Service::default()).unwrap();
    old.as_object_mut().unwrap().remove("generation");
    old.as_object_mut().unwrap().remove("restart");
    let restored: Service = serde_json::from_value(old).unwrap();
    assert!(restored.generation.is_none());
    assert!(restored.restart.is_none());
}
#[test]
fn only_the_actual_normal_restart_route_is_mediated() {
    use crate::restart_request::is_restart_path;
    assert!(is_restart_path("/v2/manager/reboot"));
    assert!(is_restart_path("/api/v2/manager/reboot"));
    for wrong in [
        "/prompt",
        "/v2/manager/reboot/other",
        "/api/api/v2/manager/reboot",
    ] {
        assert!(!is_restart_path(wrong));
    }
}
