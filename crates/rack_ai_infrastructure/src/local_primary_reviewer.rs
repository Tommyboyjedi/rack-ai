use std::time::Duration;

use rack_ai_application::ImplementationReviewer;
use rack_ai_application::ModelReviewRequest;
use rack_ai_application::ModelReviewResult;
use rack_ai_application::parse_model_review_output;
use serde_json::Value;
use serde_json::json;

pub struct LocalPrimaryReviewer {
    endpoint: String,
    model_id: String,
}

impl LocalPrimaryReviewer {
    pub fn local_default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8017/v1/chat/completions".to_string(),
            model_id: "local-primary".to_string(),
        }
    }

    pub fn new(endpoint: String, model_id: String) -> Self {
        Self {
            endpoint: normalize_endpoint(endpoint),
            model_id,
        }
    }

    fn call_api(&self, prompt: &str, timeout_seconds: u32) -> Result<String, String> {
        let payload = json!({
            "model": self.model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
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
        use sha2::{Digest, Sha256};
        let identity = format!("review-{:x}", Sha256::digest(prompt.as_bytes()));
        let mut response = agent
            .post(&self.endpoint)
            .header("Idempotency-Key", &identity)
            .send_json(&payload)
            .map_err(|error| format!("coordinator review request failed or timed out: {error}"))?;

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
            .ok_or_else(|| "coordinator review response contained no message content".to_string())
    }
}

impl ImplementationReviewer for LocalPrimaryReviewer {
    fn review(&self, request: &ModelReviewRequest) -> Result<ModelReviewResult, String> {
        let prompt = request.prompt();
        let raw_output = self.call_api(&prompt, request.timeout_seconds)?;

        let (disposition, classification, rationale) = parse_model_review_output(&raw_output)?;

        Ok(ModelReviewResult {
            disposition,
            classification,
            rationale,
            prompt,
            raw_output,
            used_host_shell: false,
        })
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
    use super::LocalPrimaryReviewer;

    #[test]
    fn managed_dispatch_is_fenced_before_http() {
        crate::managed_dispatch_test_fixture::exercise(
            "local_primary_reviewer::tests::managed_dispatch_is_fenced_before_http",
            |endpoint| {
                let client = LocalPrimaryReviewer::new(endpoint, "local-primary".into());
                let error = client.call_api("must never dispatch", 1).unwrap_err();
                assert!(error.contains("managed endpoint requires a scoped reservation"));
            },
        );
    }

    #[test]
    fn compatibility_review_preserves_logical_identity_across_http_attempts() {
        crate::managed_dispatch_test_fixture::exercise(
            "local_primary_reviewer::tests::compatibility_review_preserves_logical_identity_across_http_attempts",
            |_| {
                use std::io::{BufRead, Read, Write};
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let port = listener.local_addr().unwrap().port();
                let root =
                    std::path::PathBuf::from(std::env::var_os("RACK_AI_RESOURCE_ROOT").unwrap());
                let path = root.join("managed.json");
                let mut document: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                document["data"]["gateway_port"] = serde_json::json!(port);
                std::fs::write(path, document.to_string()).unwrap();
                let server = std::thread::spawn(move || {
                    let mut keys = Vec::new();
                    for _ in 0..3 {
                        let (mut stream, _) = listener.accept().unwrap();
                        stream
                            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                            .unwrap();
                        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                        let mut line = String::new();
                        let mut key = None;
                        let mut size = 0;
                        loop {
                            line.clear();
                            assert!(reader.read_line(&mut line).unwrap() > 0);
                            if line == "\r\n" {
                                break;
                            }
                            if let Some((name, value)) = line.split_once(':') {
                                if name.eq_ignore_ascii_case("idempotency-key") {
                                    key = Some(value.trim().to_string());
                                }
                                if name.eq_ignore_ascii_case("content-length") {
                                    size = value.trim().parse::<usize>().unwrap();
                                }
                            }
                        }
                        let mut body = vec![0; size];
                        reader.read_exact(&mut body).unwrap();
                        keys.push(key.expect("review request omitted logical invocation identity"));
                        let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .unwrap();
                    }
                    keys
                });
                let reviewer = LocalPrimaryReviewer::new(
                    format!("http://127.0.0.1:{port}/scoped/reservation/key/v1"),
                    "fixture".into(),
                );
                assert_eq!(
                    reviewer
                        .call_api("Campaign: one; Step: review; identical evidence", 5)
                        .unwrap(),
                    "ok"
                );
                reviewer
                    .call_api("Campaign: one; Step: review; identical evidence", 5)
                    .unwrap();
                reviewer
                    .call_api("Campaign: two; Step: review; identical evidence", 5)
                    .unwrap();
                let keys = server.join().unwrap();
                assert_eq!(keys[0], keys[1]);
                assert_ne!(keys[0], keys[2]);
            },
        );
    }

    #[test]
    fn local_default_targets_primary_endpoint() {
        let reviewer = LocalPrimaryReviewer::local_default();

        assert_eq!(
            reviewer.endpoint,
            "http://127.0.0.1:8017/v1/chat/completions"
        );
        assert_eq!(reviewer.model_id, "local-primary");
    }

    #[test]
    fn normalizes_v1_endpoint() {
        let reviewer = LocalPrimaryReviewer::new(
            "http://127.0.0.1:8017/v1".to_string(),
            "local-primary".to_string(),
        );

        assert_eq!(
            reviewer.endpoint,
            "http://127.0.0.1:8017/v1/chat/completions"
        );
    }
}
