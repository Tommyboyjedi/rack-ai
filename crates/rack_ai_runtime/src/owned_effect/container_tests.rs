use super::*;
use serde_json::json;

fn observation(d: &Demand) -> serde_json::Value {
    json!({"Id":"a".repeat(64),"Name":format!("/rack-runtime-{}",d.generation),
        "Image":"sha256:fixture", "State":{"Pid":0,"Running":false,"Restarting":false,"Paused":false},
        "Config":{"Labels":{"rack.activation":d.generation},
            "Env":[format!("RACK_RUNTIME_ACTIVATION={}",d.generation)]},
        "HostConfig":{"RestartPolicy":{"Name":"no"}}})
}

#[test]
fn stopped_container_requires_complete_generation_identity() {
    let d = crate::owned_effect::tests::demand();
    let value = observation(&d);
    let c: Container = serde_json::from_value(value.clone()).unwrap();
    assert!(c.verify(&d, &"a".repeat(64)).is_ok());
    for pointer in [
        "/Name",
        "/Image",
        "/Config/Labels/rack.activation",
        "/Config/Env/0",
        "/HostConfig/RestartPolicy/Name",
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("foreign");
        let c: Container = serde_json::from_value(changed).unwrap();
        assert!(c.verify(&d, &"a".repeat(64)).is_err(), "{pointer}");
    }
}

#[test]
fn restart_pending_and_ambiguous_pid_remain_fenced() {
    let d = crate::owned_effect::tests::demand();
    for pointer in ["/State/Restarting", "/State/Paused", "/State/Running"] {
        let mut value = observation(&d);
        *value.pointer_mut(pointer).unwrap() = json!(true);
        let c: Container = serde_json::from_value(value).unwrap();
        assert!(c.verify(&d, &"a".repeat(64)).is_err(), "{pointer}");
    }
}

#[test]
fn saved_container_identity_cannot_switch_or_omit_required_evidence() {
    let mut d = crate::owned_effect::tests::demand();
    d.process = Some(crate::types::Process {
        pid: 1,
        boot: "fixture".into(),
        start: "1".into(),
        activation: d.generation.clone(),
        unit: None,
        container: Some("b".repeat(64)),
        invocation: None,
    });
    let value = observation(&d);
    let c: Container = serde_json::from_value(value.clone()).unwrap();
    assert!(c.verify(&d, &"a".repeat(64)).is_err());
    let mut missing = value;
    missing.as_object_mut().unwrap().remove("HostConfig");
    assert!(serde_json::from_value::<Container>(missing).is_err());
}
