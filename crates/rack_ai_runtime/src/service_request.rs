use crate::{
    config::{Backend, Profile, ReasoningDefault},
    protocol::{Payload, Protocol},
};
use serde_json::{Map, Value, json};

pub const MODEL_ALIAS: &str = "rackai-service";

pub struct NormalizeInput<'a> {
    pub profile: &'a Profile,
    pub protocol: Protocol,
    pub body: Value,
}

pub fn normalize(input: NormalizeInput<'_>) -> Result<Payload, String> {
    let NormalizeInput {
        profile,
        protocol,
        mut body,
    } = input;
    let object = body.as_object_mut().ok_or("invalid_protocol_body")?;
    let requested_model = object
        .get("model")
        .and_then(Value::as_str)
        .ok_or("protocol_or_model_unqualified")?;
    let client_model = if requested_model == MODEL_ALIAS {
        object.insert("model".into(), Value::String(profile.model.clone()));
        Some(MODEL_ALIAS.to_string())
    } else if requested_model == profile.model {
        None
    } else {
        return Err("protocol_or_model_unqualified".into());
    };
    if protocol == Protocol::ChatCompletions {
        normalize_reasoning(profile, object)?;
    }
    Ok(Payload {
        protocol,
        body,
        client_model,
    })
}

fn normalize_reasoning(profile: &Profile, body: &mut Map<String, Value>) -> Result<(), String> {
    if body.contains_key("chat_template_kwargs") {
        if body.contains_key("reasoning_effort") {
            return Err("ambiguous_reasoning_control".into());
        }
        return Ok(());
    }
    if let Some(value) = body.remove("reasoning_effort") {
        return explicit_reasoning(profile, body, value);
    }
    if profile.reasoning_default != Some(ReasoningDefault::Disabled) {
        return Ok(());
    }
    match profile.backend {
        Backend::LlamaCpp | Backend::Strata => {
            body.insert(
                "chat_template_kwargs".into(),
                json!({"enable_thinking": false}),
            );
        }
        Backend::Ninfer => {
            body.insert("reasoning_effort".into(), json!("none"));
        }
        _ => {}
    }
    Ok(())
}

fn explicit_reasoning(
    profile: &Profile,
    body: &mut Map<String, Value>,
    value: Value,
) -> Result<(), String> {
    let effort = value
        .as_str()
        .filter(|effort| ["none", "low", "medium", "high"].contains(effort))
        .ok_or("invalid_reasoning_effort")?;
    match profile.backend {
        Backend::LlamaCpp => {
            body.insert(
                "chat_template_kwargs".into(),
                json!({"reasoning_effort": effort}),
            );
        }
        Backend::Strata if effort == "none" => {
            body.insert(
                "chat_template_kwargs".into(),
                json!({"enable_thinking": false}),
            );
        }
        Backend::Strata => return Err("reasoning_semantics_unrepresentable".into()),
        _ => {
            body.insert("reasoning_effort".into(), Value::String(effort.into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Driver, InputTokenCounter};
    use std::{collections::BTreeMap, path::PathBuf};

    fn profile(backend: Backend) -> Profile {
        Profile {
            tag: "service".into(),
            version: "v1".into(),
            model: "backend-model".into(),
            backend,
            driver: Driver::Fixture,
            qualified: true,
            evidence: vec!["test".into()],
            capabilities: vec![],
            context_tokens: 4096,
            context_reserve_tokens: 0,
            launch: None,
            max_input_tokens: 3000,
            protocols: vec![Protocol::ChatCompletions],
            streaming: true,
            input_token_preflight: false,
            input_token_counter: None::<InputTokenCounter>,
            reasoning_default: Some(ReasoningDefault::Disabled),
            enforce_input_limits: false,
            max_images_per_request: 0,
            max_image_bytes: 0,
            max_image_pixels: 0,
            image_mime_types: None,
            max_output_tokens: 512,
            resources: vec!["gpu".into()],
            device_mib: BTreeMap::from([("gpu".into(), 1)]),
            host_mib: 1,
            cpu_percent: 1,
            endpoint: "http://127.0.0.1:1".into(),
            executable: PathBuf::from("/bin/true"),
            container_image: None,
            container_mounts: BTreeMap::new(),
            executable_sha256: "0".repeat(64),
            args: vec![],
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
        }
    }

    fn request(backend: Backend, extra: Value) -> Payload {
        let mut body = json!({"model":MODEL_ALIAS,"messages":[{"role":"user","content":"hi"}]});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        normalize(NormalizeInput {
            profile: &profile(backend),
            protocol: Protocol::ChatCompletions,
            body,
        })
        .unwrap()
    }

    #[test]
    fn one_alias_maps_to_backend_identity_with_backend_specific_default() {
        let llama = request(Backend::LlamaCpp, json!({}));
        assert_eq!(llama.body["model"], "backend-model");
        assert_eq!(llama.client_model.as_deref(), Some(MODEL_ALIAS));
        assert_eq!(
            llama.body["chat_template_kwargs"],
            json!({"enable_thinking":false})
        );
        let ninfer = request(Backend::Ninfer, json!({}));
        assert_eq!(ninfer.body["reasoning_effort"], "none");
        assert!(ninfer.body.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn explicit_semantics_win_and_translate_or_fail_closed() {
        let llama = request(Backend::LlamaCpp, json!({"reasoning_effort":"high"}));
        assert_eq!(
            llama.body["chat_template_kwargs"],
            json!({"reasoning_effort":"high"})
        );
        let ninfer = request(Backend::Ninfer, json!({"reasoning_effort":"high"}));
        assert_eq!(ninfer.body["reasoning_effort"], "high");
        let result = normalize(NormalizeInput {
            profile: &profile(Backend::Strata),
            protocol: Protocol::ChatCompletions,
            body: json!({"model":MODEL_ALIAS,"messages":[],"reasoning_effort":"high"}),
        });
        assert_eq!(result.unwrap_err(), "reasoning_semantics_unrepresentable");
    }

    #[test]
    fn existing_backend_model_remains_compatible() {
        let p = profile(Backend::Vllm);
        let payload = normalize(NormalizeInput {
            profile: &p,
            protocol: Protocol::ChatCompletions,
            body: json!({"model":"backend-model","messages":[]}),
        })
        .unwrap();
        assert!(payload.client_model.is_none());
        assert_eq!(payload.body["model"], "backend-model");
    }
}
