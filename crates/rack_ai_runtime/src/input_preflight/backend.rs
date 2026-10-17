use crate::{protocol::Payload, service::Service, types::Demand};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{
    io::Read,
    time::{Duration, Instant},
};
const TIMEOUT_SECONDS: u64 = 10;
const CONNECT_SECONDS: u64 = 2;

struct Counter {
    client: Client,
    endpoint: String,
    deadline: Instant,
    max_bytes: u64,
}
impl Counter {
    fn new(service: &Service, demand: &Demand) -> Result<Self, String> {
        let duration = Duration::from_secs(TIMEOUT_SECONDS);
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(CONNECT_SECONDS))
                .timeout(duration)
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .map_err(|_| "input_preflight_backend_failure")?,
            endpoint: demand.profile.endpoint.trim_end_matches('/').into(),
            deadline: Instant::now() + duration,
            max_bytes: service.config.limits.max_response_bytes,
        })
    }
    fn post(&self, input: (&str, &Value)) -> Result<Value, String> {
        let (path, body) = input;
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or("input_preflight_backend_failure")?;
        let response = self
            .client
            .post(format!("{}{path}", self.endpoint))
            .json(body)
            .timeout(remaining)
            .send()
            .map_err(|_| "input_preflight_backend_failure")?;
        if !response.status().is_success() {
            return Err("input_preflight_backend_failure".into());
        }
        let mut bytes = Vec::new();
        response
            .take(self.max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "input_preflight_backend_failure")?;
        if bytes.len() as u64 > self.max_bytes {
            return Err("input_preflight_response_oversized".into());
        }
        serde_json::from_slice(&bytes).map_err(|_| "input_preflight_backend_invalid_json".into())
    }
}

pub(super) fn count(service: &Service, input: (&Demand, &Payload)) -> Result<u32, String> {
    let (demand, payload) = input;
    let counter = Counter::new(service, demand)?;
    let rendered = counter.post(("/apply-template", &payload.body))?;
    let prompt = rendered
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|prompt| !prompt.is_empty())
        .ok_or("input_preflight_invalid_template")?;
    // Match llama.cpp chat dispatch: both special-token switches must be explicit.
    let tokenized = counter.post((
        "/tokenize",
        &json!({
            "content": prompt, "add_special": true, "parse_special": true, "with_pieces": false,
        }),
    ))?;
    token_count(&tokenized)
}

fn token_count(value: &Value) -> Result<u32, String> {
    let tokens = value
        .get("tokens")
        .and_then(Value::as_array)
        .filter(|tokens| !tokens.is_empty())
        .ok_or("input_preflight_invalid_tokens")?;
    if tokens
        .iter()
        .any(|token| token.as_u64().is_none_or(|id| id > i32::MAX as u64))
    {
        return Err("input_preflight_invalid_tokens".into());
    }
    u32::try_from(tokens.len()).map_err(|_| "input_preflight_invalid_tokens".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_integer_tokens_without_measuring_utf8_bytes() {
        assert_eq!(token_count(&json!({"tokens":[1,2,3]})).unwrap(), 3);
    }
    #[test]
    fn rejects_missing_empty_or_malformed_tokenizer_results() {
        for value in [
            json!({}),
            json!({"tokens":[]}),
            json!({"tokens":[true]}),
            json!({"tokens":[-1]}),
            json!({"tokens":[1.5]}),
            json!({"tokens":[2147483648u64]}),
        ] {
            assert!(token_count(&value).is_err());
        }
    }
}
