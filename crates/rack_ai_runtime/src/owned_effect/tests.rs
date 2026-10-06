use super::*;
use serde_json::json;

pub(super) fn demand() -> Demand {
    serde_json::from_value(json!({
        "id":"owned-effect", "owner":"fixture",
        "request":{"schema":VERSION,"source_system":"fixture","work_id":"cleanup",
            "acquisition_id":"acquire","tag":"test","priority":"low","capabilities":[],
            "context_tokens":1,"ttl_seconds":60,"qualification":true},
        "priority":"low", "profile":{
            "tag":"test","version":"fixture","model":"test","backend":"vllm",
            "driver":"fixture","qualified":true,"evidence":[],"capabilities":[],
            "context_tokens":1,"max_input_tokens":1,"max_output_tokens":1,"resources":[],"device_mib":{},
            "host_mib":1,"cpu_percent":1,"endpoint":"http://127.0.0.1:1",
            "executable":"/usr/bin/sleep","executable_sha256":"","args":[],
            "artifact":null,"artifact_sha256":null,"startup_seconds":1,
            "drain_seconds":1,"stop_seconds":1,"inference_seconds":1,
            "container_image":"sha256:fixture"},
        "profile_hash":"fixture","state":"recovery_required","reason":"start_outcome_unknown",
        "generation":format!("cleanup-{}-{}",std::process::id(),crate::types::identity().unwrap()),
        "access_key":"synthetic","created":1,"order":1,"deadline":1,
        "transition_deadline":1,"victims":[],"process":null,"effect_started":true,
        "preflight_done":true,"released":true
    }))
    .unwrap()
}

#[test]
fn transient_metadata_requires_activation_restart_policy_and_transient_ownership() {
    let good = "Transient=yes\nRestart=no\nEnvironment=CUDA_VISIBLE_DEVICES=fixture RACK_RUNTIME_ACTIVATION=owned";
    assert!(systemd::validate_metadata(good, "owned").is_ok());
    for bad in [
        good.replace("Transient=yes", "Transient=no"),
        good.replace("Restart=no", "Restart=always"),
        good.replace("=owned", "=foreign"),
        good.replace("Environment=", "Missing="),
    ] {
        assert!(systemd::validate_metadata(&bad, "owned").is_err());
    }
}

#[test]
fn saved_fixture_process_is_verified_and_not_replayed() {
    let mut d = demand();
    let mut child = std::process::Command::new(&d.profile.executable)
        .arg("30")
        .env("RACK_RUNTIME_ACTIVATION", &d.generation)
        .spawn()
        .unwrap();
    let readiness = wait_for_activation(child.id(), &d.generation);
    if let Err(error) = readiness {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("fixture activation did not become visible: {error}");
    }
    d.process = Some(crate::process::capture(child.id(), &d.generation).unwrap());
    let result = resolve(&d);
    let pid = child.id();
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(result.unwrap().unwrap().pid, pid);
    assert!(resolve(&d).unwrap().is_none());
    assert!(absent(&d).is_ok());
}

#[test]
fn fixture_saved_process_generation_change_is_fenced() {
    let mut d = demand();
    let mut child = std::process::Command::new(&d.profile.executable)
        .arg("30")
        .env("RACK_RUNTIME_ACTIVATION", &d.generation)
        .spawn()
        .unwrap();
    let readiness = wait_for_activation(child.id(), &d.generation);
    if let Err(error) = readiness {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("fixture activation did not become visible: {error}");
    }
    d.process = Some(crate::process::capture(child.id(), &d.generation).unwrap());
    d.process.as_mut().unwrap().start.push('0');
    let result = resolve(&d);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(result.unwrap_err(), "recovery_fixture_process_changed");
}

// A successful exec can close the spawn handshake before /proc exposes env_end.
// Wait for fixture readiness, then invoke the production recovery check once.
fn wait_for_activation(pid: u32, generation: &str) -> Result<(), String> {
    let expected = format!("RACK_RUNTIME_ACTIVATION={generation}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let environment =
            std::fs::read(format!("/proc/{pid}/environ")).map_err(|e| e.to_string())?;
        if environment
            .split(|byte| *byte == 0)
            .any(|value| value == expected.as_bytes())
        {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err("activation readiness deadline".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
