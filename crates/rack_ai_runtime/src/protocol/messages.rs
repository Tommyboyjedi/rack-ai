//! Standard function-tool turns share one validation policy with token counting.
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: Function,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Function {
    name: String,
    arguments: String,
}

pub(super) fn validate(message: &Value) -> Result<bool, String> {
    if message.get("role").and_then(Value::as_str) == Some("tool")
        && message
            .get("tool_call_id")
            .and_then(Value::as_str)
            .is_none_or(|id| id.trim().is_empty())
    {
        return Err("invalid_protocol_tool_message".into());
    }
    let Some(calls) = message.get("tool_calls").filter(|calls| !calls.is_null()) else {
        return Ok(false);
    };
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return Err("invalid_protocol_tool_calls".into());
    }
    let calls = calls.as_array().ok_or("invalid_protocol_tool_calls")?;
    let mut ids = BTreeSet::new();
    for call in calls {
        let call: ToolCall =
            serde_json::from_value(call.clone()).map_err(|_| "invalid_protocol_tool_calls")?;
        if call.kind != "function"
            || call.id.trim().is_empty()
            || call.function.name.trim().is_empty()
            || !ids.insert(call.id)
        {
            return Err("invalid_protocol_tool_calls".into());
        }
        // Arguments are opaque JSON text; interpreting or executing them belongs to the client.
        let _ = call.function.arguments;
    }
    Ok(!calls.is_empty())
}

pub(crate) fn text_message(message: &Value) -> bool {
    message.is_object()
        && message.get("role").is_some_and(Value::is_string)
        && validate(message).is_ok_and(|tool_calls| {
            message.get("content").is_some_and(Value::is_string)
                || (tool_calls && message.get("content").is_none_or(Value::is_null))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_message() -> Value {
        json!({"role":"assistant", "content":null, "tool_calls":[{
            "id":"call-1", "type":"function", "function":{"name":"lookup", "arguments":"{\"key\":\"A\"}"}
        }]})
    }

    #[test]
    fn accepts_assistant_null_or_omitted_content_only_with_valid_tool_calls() {
        let mut message = tool_message();
        let original = message.clone();
        assert!(text_message(&message));
        assert_eq!(message, original);
        message.as_object_mut().unwrap().remove("content");
        assert!(text_message(&message));
        for content in [json!(null), json!(true), json!(17), json!([])] {
            assert!(!text_message(
                &json!({"role":"assistant", "content":content})
            ));
        }
    }

    #[test]
    fn rejects_malformed_tool_calls_and_non_assistant_tool_calls() {
        for malformed in [
            json!({}),
            json!(false),
            json!([{}]),
            json!([{
                "id":"call-1", "type":"function", "function":{"name":"lookup", "arguments":{}}
            }]),
            json!([{
                "id":"call-1", "type":"shell", "function":{"name":"lookup", "arguments":"{}"}
            }]),
        ] {
            let mut message = tool_message();
            message["tool_calls"] = malformed;
            assert!(validate(&message).is_err());
            assert!(!text_message(&message));
        }
        let mut message = tool_message();
        message["role"] = json!("user");
        assert!(validate(&message).is_err());
        message["role"] = json!("assistant");
        message["tool_calls"][0]["id"] = json!("");
        assert!(validate(&message).is_err());
    }

    #[test]
    fn preserves_tool_results_but_rejects_missing_ids_and_media() {
        let result = json!({"role":"tool", "tool_call_id":"call-1", "content":"{\"value\":3}"});
        assert!(text_message(&result));
        assert!(!text_message(&json!({"role":"tool", "content":"result"})));
        assert!(!text_message(
            &json!({"role":"user", "content":[{"type":"image_url"}]})
        ));
        let mut duplicate = tool_message();
        let call = duplicate["tool_calls"][0].clone();
        duplicate["tool_calls"].as_array_mut().unwrap().push(call);
        assert!(validate(&duplicate).is_err());
    }
}
