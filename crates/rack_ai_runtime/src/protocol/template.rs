use super::{Payload, Protocol};
use crate::config::Backend;
use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatTemplateOptions {
    #[serde(default, deserialize_with = "present_value")]
    enable_thinking: Option<bool>,
    #[serde(default, deserialize_with = "present_value")]
    reasoning_effort: Option<ReasoningEffort>,
}

// Missing optional fields are allowed; explicitly supplied null values are not.
fn present_value<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

pub(super) fn validate(payload: &Payload, backend: &Backend) -> Result<(), String> {
    let Some(options) = payload.body.get("chat_template_kwargs") else {
        return Ok(());
    };
    if payload.protocol != Protocol::ChatCompletions
        || !matches!(backend, Backend::LlamaCpp | Backend::Strata)
    {
        return Err("chat_template_kwargs_unqualified".into());
    }
    let options: ChatTemplateOptions =
        serde_json::from_value(options.clone()).map_err(|_| "invalid_chat_template_kwargs")?;
    if options.enable_thinking.is_none() && options.reasoning_effort.is_none() {
        return Err("invalid_chat_template_kwargs".into());
    }
    if *backend == Backend::Strata && options.reasoning_effort.is_some() {
        return Err("chat_template_kwargs_unqualified".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(options: serde_json::Value) -> Payload {
        Payload {
            protocol: Protocol::ChatCompletions,
            body: json!({"chat_template_kwargs": options}),
        }
    }

    #[test]
    fn accepts_explicit_boolean_thinking_control_for_supported_backends() {
        for enabled in [false, true] {
            let payload = payload(json!({"enable_thinking": enabled}));
            let original = payload.body.clone();
            assert!(validate(&payload, &Backend::LlamaCpp).is_ok());
            assert!(validate(&payload, &Backend::Strata).is_ok());
            assert_eq!(payload.body, original);
        }
    }

    #[test]
    fn preserves_native_llama_template_effort_with_or_without_thinking_boolean() {
        for effort in ["none", "low", "medium", "high"] {
            for options in [
                json!({"reasoning_effort":effort}),
                json!({"enable_thinking":false,"reasoning_effort":effort}),
            ] {
                let payload = payload(options);
                let original = payload.body.clone();
                assert!(validate(&payload, &Backend::LlamaCpp).is_ok());
                assert_eq!(payload.body, original);
                assert_eq!(
                    validate(&payload, &Backend::Strata).unwrap_err(),
                    "chat_template_kwargs_unqualified"
                );
            }
        }
    }

    #[test]
    fn rejects_malformed_or_unqualified_template_options() {
        for options in [
            json!(null),
            json!(false),
            json!([]),
            json!({}),
            json!({"enable_thinking": "false"}),
            json!({"enable_thinking": 0}),
            json!({"enable_thinking": null}),
            json!({"enable_thinking": false, "other": true}),
            json!({"reasoning_effort":null}),
            json!({"reasoning_effort":false}),
            json!({"reasoning_effort":0}),
            json!({"reasoning_effort":"unknown"}),
            json!({"reasoning_effort":"HIGH"}),
            json!({"enable_thinking":null,"reasoning_effort":"none"}),
            json!({"enable_thinking":false,"reasoning_effort":null}),
        ] {
            assert_eq!(
                validate(&payload(options), &Backend::LlamaCpp).unwrap_err(),
                "invalid_chat_template_kwargs"
            );
        }
    }

    #[test]
    fn rejects_template_options_on_other_backends_and_protocols() {
        let mut payload = payload(json!({"enable_thinking": false}));
        for backend in [Backend::Vllm, Backend::Comfyui, Backend::Chatterbox] {
            assert_eq!(
                validate(&payload, &backend).unwrap_err(),
                "chat_template_kwargs_unqualified"
            );
        }
        for protocol in [Protocol::Responses, Protocol::Speech] {
            payload.protocol = protocol;
            assert_eq!(
                validate(&payload, &Backend::LlamaCpp).unwrap_err(),
                "chat_template_kwargs_unqualified"
            );
        }
    }

    #[test]
    fn preserves_existing_requests_without_template_options() {
        let payload = Payload {
            protocol: Protocol::ChatCompletions,
            body: json!({"model":"existing", "response_format":{"type":"json_object"}}),
        };
        assert!(validate(&payload, &Backend::Vllm).is_ok());
        assert!(validate(&payload, &Backend::LlamaCpp).is_ok());
    }
}
