//! Count only content admitted by the frozen protocol and native counter.
use crate::{
    protocol::{Payload, Protocol},
    types::Demand,
};
use serde_json::Value;

pub(super) fn validate(payload: &Payload, demand: &Demand) -> Result<(), String> {
    let messages = payload
        .body
        .get("messages")
        .and_then(Value::as_array)
        .filter(|messages| !messages.is_empty())
        .ok_or("input_preflight_unsupported_content")?;
    if payload.protocol != Protocol::ChatCompletions
        || messages
            .iter()
            .any(|message| !countable(message, demand.profile.native_multimodal_counter()))
    {
        return Err("input_preflight_unsupported_content".into());
    }
    Ok(())
}
fn countable(message: &Value, native: bool) -> bool {
    if crate::protocol::text_message(message) {
        return true;
    }
    if !native {
        return false;
    }
    let role = message.get("role").and_then(Value::as_str);
    if !matches!(
        role,
        Some("system" | "developer" | "user" | "assistant" | "tool")
    ) {
        return false;
    }
    let Some(parts) = message
        .get("content")
        .and_then(Value::as_array)
        .filter(|p| !p.is_empty())
    else {
        return false;
    };
    parts
        .iter()
        .all(|part| match part.get("type").and_then(Value::as_str) {
            Some("text") => part.get("text").is_some_and(Value::is_string),
            Some("image_url") if role == Some("user") => {
                part.get("image_url").is_some_and(image_options)
            }
            _ => false,
        })
}
fn image_options(image: &Value) -> bool {
    image.as_object().is_some_and(|object| {
        object.get("url").is_some_and(Value::is_string)
            && object
                .keys()
                .all(|key| matches!(key.as_str(), "url" | "detail"))
            && object
                .get("detail")
                .is_none_or(|detail| detail.as_str() == Some("auto"))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_accepts_standard_text_and_user_image_arrays_only() {
        let image = json!({"role":"user","content":[{"type":"text","text":"Describe"},
            {"type":"image_url","image_url":{"url":"already-protocol-validated","detail":"auto"}}]});
        assert!(countable(&image, true));
        assert!(!countable(&image, false));
        let mut changed = image.clone();
        changed["role"] = json!("assistant");
        assert!(!countable(&changed, true));
        for detail in [json!("high"), json!("low"), json!(null), json!(true)] {
            changed = image.clone();
            changed["content"][1]["image_url"]["detail"] = detail;
            assert!(!countable(&changed, true));
        }
        assert!(!countable(&json!({"role":"user","content":[]}), true));
        assert!(!countable(
            &json!({"role":"user","content":[{"type":"audio"}]}),
            true
        ));
        assert!(countable(&json!({"role":"user","content":"legacy"}), false));
    }
}
