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

    fn call_api(&self, prompt: &str) -> Result<String, String> {
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

        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_send_request(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(300)))
            .timeout_global(Some(Duration::from_secs(330)))
            .build();

        let agent = config.new_agent();

        let mut response = agent
            .post(&self.endpoint)
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
        let raw_output = self.call_api(&prompt)?;

        let (disposition, classification, rationale) =
            parse_model_review_output(&raw_output)?;

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