use std::path::Path;
use std::time::Duration;

use base64::Engine;
use serde_json::Value;
use serde_json::json;

pub struct LocalPrimaryChat {
    endpoint: String,
    model_id: String,
}

impl LocalPrimaryChat {
    pub fn new(endpoint: String, model_id: String) -> Self {
        Self {
            endpoint: normalize_endpoint(endpoint),
            model_id,
        }
    }

    pub fn send(
        &self,
        prompt: &str,
        image_sources: &[String],
        image_root: &Path,
        timeout_seconds: u32,
        idempotency_key: Option<&str>,
    ) -> Result<String, String> {
        if prompt.trim().is_empty() {
            return Err("Prompt is empty.".to_string());
        }
        let content = user_content(prompt, image_sources, image_root)?;
        let payload = json!({
            "model": self.model_id,
            "messages": [
                {
                    "role": "user",
                    "content": content
                }
            ],
            "stream": false,
            "temperature": 0,
        });

        let global = Duration::from_secs(u64::from(timeout_seconds.max(1)));
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_send_request(Some(global))
            .timeout_recv_response(Some(global))
            .timeout_global(Some(global))
            .build();
        let agent = config.new_agent();
        let _dispatch = crate::endpoint_fence::EndpointFence::local(&self.endpoint)?;
        let identity = idempotency_key
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| derived_identity(&payload));
        let mut response = agent
            .post(&self.endpoint)
            .header("Idempotency-Key", &identity)
            .send_json(&payload)
            .map_err(|error| format!("local-primary chat request failed or timed out: {error}"))?;
        let response = response
            .body_mut()
            .read_json::<Value>()
            .map_err(|error| error.to_string())?;
        response
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "local-primary response contained no message content".to_string())
    }
}

fn user_content(
    prompt: &str,
    image_sources: &[String],
    image_root: &Path,
) -> Result<Value, String> {
    if image_sources.is_empty() {
        return Ok(json!(prompt));
    }
    let mut parts = vec![json!({"type": "text", "text": prompt})];
    for source in image_sources {
        let url = image_url(source, image_root)?;
        parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
    }
    Ok(json!(parts))
}

fn image_url(source: &str, image_root: &Path) -> Result<String, String> {
    if source.starts_with("data:image/") {
        return Ok(source.to_string());
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        return Err("remote image URLs are not accepted by the managed gateway; pass a local image file or data:image URI".to_string());
    }
    let path = Path::new(source);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        image_root.join(path)
    };
    let bytes = std::fs::read(&path).map_err(|error| {
        format!(
            "failed to read image attachment {}: {error}",
            path.to_string_lossy()
        )
    })?;
    let mime_type = infer_image_mime_type(&path).ok_or_else(|| {
        format!(
            "image attachment {} needs a known image extension",
            path.to_string_lossy()
        )
    })?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime_type};base64,{encoded}"))
}

fn derived_identity(payload: &Value) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(payload).unwrap_or_default();
    format!("primary-image-{:x}", Sha256::digest(bytes))
}

fn infer_image_mime_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

fn normalize_endpoint(endpoint: String) -> String {
    if endpoint.ends_with("/chat/completions") {
        endpoint
    } else {
        format!("{}/chat/completions", endpoint.trim_end_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::user_content;

    #[test]
    fn builds_multimodal_content_from_local_image() {
        let root = temp_root();
        std::fs::write(root.join("screen.png"), b"png-bytes").unwrap();
        let content = user_content("Inspect.", &["screen.png".to_string()], &root).unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Inspect.");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"].as_str().unwrap(),
            "data:image/png;base64,cG5nLWJ5dGVz"
        );
    }

    #[test]
    fn rejects_remote_image_urls() {
        let root = temp_root();
        let error = user_content(
            "Inspect.",
            &["https://example.test/screen.webp".to_string()],
            &root,
        )
        .unwrap_err();
        assert!(error.contains("remote image URLs are not accepted"));
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-local-primary-chat-{nanos}"));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}
