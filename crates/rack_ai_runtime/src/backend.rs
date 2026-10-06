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
        if d.profile.backend == Backend::Chatterbox {
            return crate::speech_backend::ready(d);
        }
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
}
pub fn infer(
    service: &crate::service::Service,
    input: (&Demand, &Invocation),
) -> Result<Value, String> {
    let (d, invocation) = input;
    let config = &service.config;
    let request = &invocation.request;
    process::endpoint_owned(
        d.process.as_ref().ok_or("activation_process_missing")?,
        &d.profile,
    )?;
    if d.profile.backend == Backend::Chatterbox {
        return crate::speech_backend::synthesize((d, invocation), &config.authority_root);
    }
    let client = client(request.timeout_seconds)?;
    let body = request
        .payload
        .as_ref()
        .map(|p| std::borrow::Cow::Borrowed(&p.body))
        .unwrap_or_else(|| {
            std::borrow::Cow::Owned(json!({
        "model":d.profile.model,"messages":[{"role":"user","content":request.prompt}],
        "max_tokens":request.max_tokens,"stream":false}))
        });
    let path = request
        .payload
        .as_ref()
        .map_or("/v1/chat/completions", |p| p.path());
    let call = client
        .post(format!(
            "{}{}",
            d.profile.endpoint.trim_end_matches('/'),
            path
        ))
        .json(&body);
    let mut capture = crate::interaction_capture::begin(service, (d, invocation, &body));
    if let Some(capture) = capture.as_mut() {
        capture.dispatched();
    }
    let mut observed = None;
    let result = receive(call, (invocation, capture.is_some(), &mut observed));
    if let Some(capture) = capture {
        capture.finish((observed, &result));
    }
    result
}

fn receive(
    call: reqwest::blocking::RequestBuilder,
    input: (&Invocation, bool, &mut Option<Value>),
) -> Result<Value, String> {
    let (invocation, capture_enabled, observed) = input;
    let response = call.send().map_err(|_| "backend_transport_uncertain")?;
    let status = response.status();
    if !status.is_success() && !capture_enabled {
        return Err(http_error(invocation, status));
    }
    let mut bytes = Vec::new();
    let read = response
        .take(invocation.response_bytes + 1)
        .read_to_end(&mut bytes);
    if capture_enabled {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        *observed = Some(serde_json::from_str(&text).unwrap_or(Value::String(text)));
    }
    if !status.is_success() {
        return Err(http_error(invocation, status));
    }
    read.map_err(|_| "backend_read_uncertain")?;
    if bytes.len() as u64 > invocation.response_bytes {
        return Err("backend_response_oversized".into());
    }
    if let Some(payload) = &invocation.request.payload {
        crate::protocol::raw_result(
            String::from_utf8(bytes).map_err(|_| "invalid_protocol_encoding")?,
            payload,
        )
    } else {
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }
}

fn http_error(invocation: &Invocation, status: reqwest::StatusCode) -> String {
    if invocation.request.payload.is_some() {
        "backend_http_failure".into()
    } else {
        format!("backend_http_{status}")
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
