use crate::{config::Backend, process, types::*};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{io::Read, time::Duration};
const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
pub struct BackendAccess<'a> {
    pub config: &'a crate::config::Config,
}
impl BackendAccess<'_> {
    pub fn ready(&self, d: &Demand) -> Result<(), String> {
        if d.profile.backend == Backend::Comfyui
            && d.profile.driver != crate::config::Driver::Fixture
        {
            return crate::media::MediaAdapter {
                config: self.config,
            }
            .ready(d);
        }
        let process = d.process.as_ref().ok_or("activation_process_missing")?;
        process::endpoint_owned(process, &d.profile)?;

        let client = client(2)?;
        let body = decode(
            client
                .get(format!(
                    "{}/v1/models",
                    d.profile.endpoint.trim_end_matches('/')
                ))
                .send(),
            MAX_RESPONSE,
        )?;
        let ids: Vec<_> = body
            .get("data")
            .and_then(Value::as_array)
            .ok_or("invalid_model_identity")?
            .iter()
            .filter_map(|m| m.get("id").and_then(Value::as_str))
            .collect();
        if ids != vec![d.profile.model.as_str()] {
            return Err("model_identity_mismatch".into());
        }
        process::endpoint_owned(process, &d.profile)
    }
    pub fn infer(&self, d: &Demand, invocation: &Invocation) -> Result<Value, String> {
        // The caller persisted Started and revalidated the generation before entering.
        // No automatic retry is allowed after this boundary.
        let request = &invocation.request;
        let path = request
            .payload
            .as_ref()
            .map_or(Ok("/v1/chat/completions"), |p| p.path())?;
        process::endpoint_owned(
            d.process.as_ref().ok_or("activation_process_missing")?,
            &d.profile,
        )?;
        let bound = invocation.response_bytes;
        if let Some(payload) = &request.payload {
            let response = client(request.timeout_seconds)?
                .post(format!(
                    "{}{}",
                    d.profile.endpoint.trim_end_matches('/'),
                    path
                ))
                .json(&payload.body)
                .send()
                .map_err(|_| "backend_transport_uncertain")?;
            if !response.status().is_success() {
                return Err("backend_http_failure".into());
            }
            let mut bytes = Vec::new();
            response
                .take(bound + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "backend_read_uncertain")?;
            if bytes.len() as u64 > bound {
                return Err("backend_response_oversized".into());
            }
            return crate::protocol::raw_result(
                String::from_utf8(bytes).map_err(|_| "invalid_protocol_encoding")?,
                payload,
            );
        }
        decode(client(request.timeout_seconds)?.post(format!("{}/v1/chat/completions", d.profile.endpoint.trim_end_matches('/')))
            .json(&json!({"model": d.profile.model, "messages": [{"role":"user","content":request.prompt}],
                "max_tokens":request.max_tokens,"stream":false})).send(), bound)
    }
}
fn client(seconds: u64) -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(seconds))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())
}
fn decode(
    response: Result<reqwest::blocking::Response, reqwest::Error>,
    bound: u64,
) -> Result<Value, String> {
    let response = response.map_err(|_| "backend_transport_uncertain")?;
    if !response.status().is_success() {
        return Err(format!("backend_http_{}", response.status()));
    }
    let mut bytes = Vec::new();
    response
        .take(bound + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "backend_read_uncertain")?;
    if bytes.len() as u64 > bound {
        return Err("backend_response_oversized".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "backend_protocol_uncertain".into())
}
