use std::time::Duration;

use rack_ai_application::RecoveryReasoner;
use rack_ai_application::RecoveryReasoningRequest;
use rack_ai_application::RecoveryReasoningResult;
use rack_ai_application::parse_recovery_output;
use serde_json::Value;
use serde_json::json;

pub struct LocalPrimaryRecoveryReasoner {
    endpoint: String,
    model_id: String,
}

impl LocalPrimaryRecoveryReasoner {
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

        let mut response = agent
            .post(&self.endpoint)
            .send_json(&payload)
            .map_err(|error| format!("recovery diagnosis request failed or timed out: {error}"))?;

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
            .ok_or_else(|| "recovery diagnosis response contained no message content".to_string())
    }
}

impl RecoveryReasoner for LocalPrimaryRecoveryReasoner {
    fn diagnose(
        &self,
        request: &RecoveryReasoningRequest,
    ) -> Result<RecoveryReasoningResult, String> {
        let prompt = request.prompt();
        let raw_output = self.call_api(&prompt, request.timeout_seconds())?;
        let decision = parse_recovery_output(&raw_output)?;
        Ok(RecoveryReasoningResult {
            decision,
            prompt,
            raw_output,
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
    use super::LocalPrimaryRecoveryReasoner;

    #[test]
    fn local_default_targets_primary_endpoint() {
        let reasoner = LocalPrimaryRecoveryReasoner::local_default();

        assert_eq!(
            reasoner.endpoint,
            "http://127.0.0.1:8017/v1/chat/completions"
        );
        assert_eq!(reasoner.model_id, "local-primary");
    }

    #[test]
    fn normalizes_v1_endpoint() {
        let reasoner = LocalPrimaryRecoveryReasoner::new(
            "http://127.0.0.1:8017/v1".to_string(),
            "local-primary".to_string(),
        );

        assert_eq!(
            reasoner.endpoint,
            "http://127.0.0.1:8017/v1/chat/completions"
        );
    }
}
