use base64::Engine;
use rack_ai_application::GenericCapability;
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
        validate_content(&self.body, self.protocol, d)?;
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

#[derive(Clone, Copy, PartialEq)]
enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
}

struct ImagePolicy<'a> {
    protocol: Protocol,
    demand: &'a crate::types::Demand,
    count: u32,
}

fn validate_content(
    body: &Value,
    protocol: Protocol,
    d: &crate::types::Demand,
) -> Result<(), String> {
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
    let mut policy = ImagePolicy {
        protocol,
        demand: d,
        count: 0,
    };
    for messages in [body.get("messages"), body.get("input")]
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
    {
        for message in messages {
            if let Some(parts) = message.get("content").and_then(Value::as_array) {
                for part in parts {
                    validate_content_part(part, &mut policy)?;
                }
            } else if message
                .get("content")
                .is_some_and(|content| !content.is_string())
            {
                return Err("invalid_protocol_content".into());
            }
        }
    }
    Ok(())
}

fn validate_content_part(part: &Value, policy: &mut ImagePolicy<'_>) -> Result<(), String> {
    match part.get("type").and_then(Value::as_str) {
        Some("text" | "input_text" | "output_text") => {
            if part.get("text").and_then(Value::as_str).is_none() {
                return Err("invalid_protocol_content".into());
            }
            Ok(())
        }
        Some("image_url") if policy.protocol == Protocol::ChatCompletions => {
            let url = part
                .get("image_url")
                .and_then(|image| {
                    image
                        .get("url")
                        .and_then(Value::as_str)
                        .or_else(|| image.as_str())
                })
                .ok_or("invalid_image_payload")?;
            validate_image_url(url, policy)
        }
        Some("image_url" | "input_image") => Err("image_input_unqualified".into()),
        _ => Err("non_text_input_unqualified".into()),
    }
}

fn validate_image_url(url: &str, policy: &mut ImagePolicy<'_>) -> Result<(), String> {
    let d = policy.demand;
    if !d.profile.capabilities.contains(&GenericCapability::Visual)
        || d.profile.max_images_per_request == 0
        || d.profile.max_image_bytes == 0
        || d.profile.max_image_pixels == 0
    {
        return Err("image_input_unqualified".into());
    }
    policy.count += 1;
    if policy.count > d.profile.max_images_per_request {
        return Err("image_count_exceeded".into());
    }
    let Some(data) = url.strip_prefix("data:") else {
        return Err("image_url_unqualified".into());
    };
    let Some((metadata, encoded)) = data.split_once(',') else {
        return Err("invalid_image_payload".into());
    };
    let Some(mime_type) = metadata.strip_suffix(";base64") else {
        return Err("invalid_image_payload".into());
    };
    let Some(declared) = declared_image_format(mime_type) else {
        return Err("unsupported_image_type".into());
    };
    if base64_upper_bound(encoded) > d.profile.max_image_bytes {
        return Err("image_bytes_exceeded".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "invalid_image_payload".to_string())?;
    if bytes.is_empty() {
        return Err("invalid_image_payload".into());
    }
    if bytes.len() as u64 > d.profile.max_image_bytes {
        return Err("image_bytes_exceeded".into());
    }
    let actual = detected_image_format(&bytes).ok_or("invalid_image_payload")?;
    if actual != declared {
        return Err("image_format_mismatch".into());
    }
    let pixels = image_pixels(&bytes, actual)?;
    if pixels == 0 || pixels > d.profile.max_image_pixels {
        return Err("image_pixels_exceeded".into());
    }
    Ok(())
}

fn base64_upper_bound(encoded: &str) -> u64 {
    let padding = encoded
        .as_bytes()
        .iter()
        .rev()
        .take_while(|c| **c == b'=')
        .count() as u64;
    ((encoded.len() as u64 + 3) / 4) * 3 - padding.min(2)
}

fn declared_image_format(mime_type: &str) -> Option<ImageFormat> {
    match mime_type {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/webp" => Some(ImageFormat::Webp),
        "image/gif" => Some(ImageFormat::Gif),
        "image/bmp" => Some(ImageFormat::Bmp),
        _ => None,
    }
}

fn detected_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(b"\xff\xd8") {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        Some(ImageFormat::Webp)
    } else if bytes.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

fn image_pixels(bytes: &[u8], format: ImageFormat) -> Result<u64, String> {
    let dimensions = match format {
        ImageFormat::Png => png_dimensions(bytes),
        ImageFormat::Jpeg => jpeg_dimensions(bytes),
        ImageFormat::Webp => webp_dimensions(bytes),
        ImageFormat::Gif => little_dimensions(bytes, 6),
        ImageFormat::Bmp => bmp_dimensions(bytes),
    }
    .ok_or("invalid_image_payload")?;
    Ok(u64::from(dimensions.0) * u64::from(dimensions.1))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 33 || &bytes[12..16] != b"IHDR" || u32_be(bytes, 8)? != 13 {
        return None;
    }
    Some((u32_be(bytes, 16)?, u32_be(bytes, 20)?))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut index = 2_usize;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xff {
            return None;
        }
        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }
        let marker = *bytes.get(index)?;
        index += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        let length = u16_be(bytes, index)? as usize;
        if length < 2 || index + length > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let height = u16_be(bytes, index + 3)?;
            let width = u16_be(bytes, index + 5)?;
            return Some((u32::from(width), u32::from(height)));
        }
        index += length;
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 {
        return None;
    }
    match &bytes[12..16] {
        b"VP8X" if bytes.len() >= 30 => Some((u24_le(bytes, 24)? + 1, u24_le(bytes, 27)? + 1)),
        b"VP8L" if bytes.len() >= 25 && bytes[20] == 0x2f => {
            let value = u32_le(bytes, 21)?;
            Some(((value & 0x3fff) + 1, ((value >> 14) & 0x3fff) + 1))
        }
        b"VP8 " if bytes.len() >= 30 && bytes[23..26] == [0x9d, 0x01, 0x2a] => Some((
            u32::from(u16_le(bytes, 26)? & 0x3fff),
            u32::from(u16_le(bytes, 28)? & 0x3fff),
        )),
        _ => None,
    }
}

fn bmp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 26 {
        return None;
    }
    let dib = u32_le(bytes, 14)?;
    if dib == 12 {
        Some((u32::from(u16_le(bytes, 18)?), u32::from(u16_le(bytes, 20)?)))
    } else if dib >= 40 && bytes.len() >= 26 {
        let width = i32_le(bytes, 18)?.unsigned_abs();
        let height = i32_le(bytes, 22)?.unsigned_abs();
        Some((width, height))
    } else {
        None
    }
}

fn little_dimensions(bytes: &[u8], offset: usize) -> Option<(u32, u32)> {
    Some((
        u32::from(u16_le(bytes, offset)?),
        u32::from(u16_le(bytes, offset + 2)?),
    ))
}

fn u16_be(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn u24_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 3)?;
    Some(u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16))
}

fn u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn i32_le(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
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
