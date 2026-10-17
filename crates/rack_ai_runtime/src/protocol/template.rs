use super::{Payload, Protocol};
use crate::config::Backend;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatTemplateOptions {
    enable_thinking: bool,
}

pub(super) fn validate(payload: &Payload, backend: &Backend) -> Result<(), String> {
    let Some(options) = payload.body.get("chat_template_kwargs") else {
        return Ok(());
    };
    if payload.protocol != Protocol::ChatCompletions || *backend != Backend::LlamaCpp {
        return Err("chat_template_kwargs_unqualified".into());
    }
    let options: ChatTemplateOptions =
        serde_json::from_value(options.clone()).map_err(|_| "invalid_chat_template_kwargs")?;
    let _ = options.enable_thinking;
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
    fn accepts_explicit_boolean_thinking_control_for_llama_cpp() {
        for enabled in [false, true] {
            let payload = payload(json!({"enable_thinking": enabled}));
            let original = payload.body.clone();
            assert!(validate(&payload, &Backend::LlamaCpp).is_ok());
            assert_eq!(payload.body, original);
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
