use super::loaded;
use serde_json::{Value, json};

fn props() -> Value {
    json!({"modalities":{"vision":true,"audio":false},
        "default_generation_settings":{"n_ctx":131072},
        "total_slots":1,"is_sleeping":false,"model_alias":"nex-n2.5-mini"})
}

#[test]
fn loaded_native_projector_and_context_are_accepted() {
    assert!(loaded(props(), 131072).is_ok());
}

#[test]
fn unloaded_or_sleeping_vision_is_rejected() {
    for (field, value) in [("vision", false), ("is_sleeping", true)] {
        let mut body = props();
        if field == "vision" {
            body["modalities"][field] = json!(value);
        } else {
            body[field] = json!(value);
        }
        assert_eq!(loaded(body, 131072).unwrap_err(), "llama_vision_not_loaded");
    }
}

#[test]
fn context_and_parallel_mismatch_are_rejected() {
    assert_eq!(
        loaded(props(), 65536).unwrap_err(),
        "llama_vision_context_mismatch"
    );
    let mut body = props();
    body["total_slots"] = json!(2);
    assert_eq!(
        loaded(body, 131072).unwrap_err(),
        "llama_vision_context_mismatch"
    );
}

#[test]
fn absent_or_malformed_vision_properties_fail_closed() {
    for body in [
        json!({}),
        json!({"modalities":{"vision":true}}),
        json!(null),
    ] {
        assert_eq!(
            loaded(body, 131072).unwrap_err(),
            "llama_vision_readiness_invalid_properties"
        );
    }
    for key in [
        "modalities",
        "default_generation_settings",
        "total_slots",
        "is_sleeping",
    ] {
        let mut body = props();
        body[key] = Value::Null;
        assert!(loaded(body, 131072).is_err());
    }
    for value in [json!("true"), json!(1), Value::Null] {
        let mut body = props();
        body["modalities"]["vision"] = value;
        assert_eq!(
            loaded(body, 131072).unwrap_err(),
            "llama_vision_readiness_invalid_properties"
        );
    }
}
