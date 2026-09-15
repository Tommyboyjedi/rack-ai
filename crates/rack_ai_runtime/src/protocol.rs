use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    ChatCompletions,
    Responses,
    Speech,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub protocol: Protocol,
    pub body: Value,
}
impl Payload {
    pub fn validate(&self, d: &crate::types::Demand) -> Result<u32, String> {
        if self.protocol == Protocol::Speech {
            return crate::speech::validate(self, d);
        }
        if d.profile.backend == crate::config::Backend::Chatterbox {
            return Err("use_speech_interface".into());
        }
        let body = self.body.as_object().ok_or("invalid_protocol_body")?;
        let allowed = match self.protocol {
            Protocol::ChatCompletions => &[
                "model",
                "messages",
                "stream",
                "max_tokens",
                "max_completion_tokens",
                "temperature",
                "top_p",
                "tools",
                "tool_choice",
                "response_format",
                "parallel_tool_calls",
                "reasoning_effort",
                "stream_options",
                "stop",
                "seed",
                "frequency_penalty",
                "presence_penalty",
            ][..],
            Protocol::Speech => &[][..],
            Protocol::Responses => &[
                "model",
                "input",
                "instructions",
                "max_output_tokens",
                "temperature",
                "top_p",
                "tools",
                "tool_choice",
                "parallel_tool_calls",
                "reasoning",
                "text",
                "stream",
                "store",
            ][..],
        };
        if body.keys().any(|k| !allowed.contains(&k.as_str()))
            || !d.profile.protocols.contains(&self.protocol)
            || body.get("model").and_then(Value::as_str) != Some(d.profile.model.as_str())
            || body.get("store").is_some_and(|v| v != false)
            || (body.get("stream").and_then(Value::as_bool) == Some(true) && !d.profile.streaming)
        {
            return Err("protocol_or_model_unqualified".into());
        }
        validate_content(&self.body)?;
        if body.contains_key("max_tokens") && body.contains_key("max_completion_tokens") {
            return Err("ambiguous_output_bound".into());
        }
        let field = match self.protocol {
            Protocol::ChatCompletions => "max_tokens",
            Protocol::Responses => "max_output_tokens",
            Protocol::Speech => return Err("use_speech_interface".into()),
        };
        let tokens = body
            .get(field)
            .or_else(|| body.get("max_completion_tokens"))
            .and_then(Value::as_u64)
            .ok_or("explicit_output_bound_required")?;
        u32::try_from(tokens).map_err(|_| "invalid_output_bound".into())
    }
    pub fn path(&self) -> &'static str {
        match self.protocol {
            Protocol::ChatCompletions => "/v1/chat/completions",
            Protocol::Responses => "/v1/responses",
            Protocol::Speech => "/speech",
        }
    }
}
pub fn default_protocols() -> Vec<Protocol> {
    vec![Protocol::ChatCompletions]
}
pub fn raw_result(bytes: String, p: &Payload) -> Result<Value, String> {
    if p.body.get("stream").and_then(Value::as_bool) == Some(true) {
        let finished = match p.protocol {
            Protocol::ChatCompletions => bytes.lines().any(|l| l.trim() == "data: [DONE]"),
            Protocol::Speech => return Err("use_speech_interface".into()),
            Protocol::Responses => bytes
                .lines()
                .any(|l| l.trim() == "event: response.completed"),
        };
        if !finished {
            return Err("stream_completion_uncertain".into());
        }
        let mut model_seen = false;
        for data in bytes.lines().filter_map(|line| line.strip_prefix("data: ")) {
            if data == "[DONE]" {
                continue;
            }
            let event: Value = serde_json::from_str(data).map_err(|_| "invalid_stream_event")?;
            let value = event.get("response").unwrap_or(&event);
            if value.get("model").is_some() {
                if value.get("model") != p.body.get("model") {
                    return Err("response_model_mismatch".into());
                }
                model_seen = true;
            }
            observed_limit(value, p)?;
        }
        if !model_seen {
            return Err("stream_model_unproven".into());
        }
        Ok(
            serde_json::json!({"rack_protocol_response":{"content_type":"text/event-stream","body":bytes}}),
        )
    } else {
        let value: Value = serde_json::from_str(&bytes).map_err(|_| "invalid_backend_json")?;
        let complete = match p.protocol {
            Protocol::ChatCompletions => value
                .get("choices")
                .and_then(Value::as_array)
                .is_some_and(|v| !v.is_empty()),
            Protocol::Speech => return Err("use_speech_interface".into()),
            Protocol::Responses => {
                value.get("status").and_then(Value::as_str) == Some("completed")
                    && value.get("output").and_then(Value::as_array).is_some()
            }
        };
        observed_limit(&value, p)?;
        if !complete || value.get("model") != p.body.get("model") {
            return Err("response_model_mismatch".into());
        }
        Ok(
            serde_json::json!({"rack_protocol_response":{"content_type":"application/json","body":bytes}}),
        )
    }
}

fn validate_content(body: &Value) -> Result<(), String> {
    if body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| tool.get("type").and_then(Value::as_str) != Some("function"))
        })
    {
        return Err("hosted_tools_unqualified".into());
    }
    for messages in [body.get("messages"), body.get("input")]
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
    {
        for message in messages {
            if let Some(parts) = message.get("content").and_then(Value::as_array)
                && parts.iter().any(|p| {
                    !matches!(
                        p.get("type").and_then(Value::as_str),
                        Some("text" | "input_text" | "output_text")
                    ) || p.get("text").and_then(Value::as_str).is_none()
                })
            {
                return Err("non_text_input_unqualified".into());
            }
        }
    }
    Ok(())
}

fn observed_limit(value: &Value, payload: &Payload) -> Result<(), String> {
    let bound = ["max_tokens", "max_completion_tokens", "max_output_tokens"]
        .iter()
        .find_map(|key| payload.body.get(key).and_then(Value::as_u64))
        .ok_or("missing_output_limit")?;
    if let Some(usage) = value.get("usage") {
        let count = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(Value::as_u64);
        if count.is_some_and(|tokens| tokens > bound) {
            return Err("backend_output_limit_exceeded".into());
        }
    }
    Ok(())
}
