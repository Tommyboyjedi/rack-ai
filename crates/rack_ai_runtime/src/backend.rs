use crate::{config::Backend, process, types::*};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{io::Read, time::Duration};
const MAX_METRICS_BYTES: u64 = 128 * 1024;
const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
pub struct BackendAccess<'a> {
    pub config: &'a crate::config::Config,
}
impl BackendAccess<'_> {
    pub(crate) fn ninfer_idle(&self, d: &Demand) -> Result<bool, String> {
        let _guard = rack_ai_infrastructure::endpoint_fence::managed_read_guard(
            &self.config.authority_root,
            &d.profile.endpoint,
        )?;
        self.ready(d)?;
        let process = d.process.as_ref().ok_or("activation_process_missing")?;
        let response = client(2)?
            .get(format!(
                "{}/metrics",
                d.profile.endpoint.trim_end_matches('/')
            ))
            .send()
            .map_err(|_| "ninfer_metrics_transport")?;
        if !response.status().is_success() {
            return Err("ninfer_metrics_http".into());
        }
        let mut metrics = String::new();
        response
            .take(MAX_METRICS_BYTES + 1)
            .read_to_string(&mut metrics)
            .map_err(|_| "ninfer_metrics_read")?;
        process::endpoint_owned(process, &d.profile)?;
        Ok(metrics.len() as u64 <= MAX_METRICS_BYTES && ninfer_metrics_idle(&metrics))
    }

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
        if d.profile.backend == Backend::Strata {
            return crate::strata_ready::ready(self.config, d);
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
        if d.profile.launch.is_some() {
            crate::launch_identity::devices(self.config, d)?;
        }
        if d.profile.backend == Backend::LlamaCpp && d.profile.native_multimodal_counter() {
            crate::llama_vision_ready::ready(d)?;
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
    let body = request
        .payload
        .as_ref()
        .map(|p| std::borrow::Cow::Borrowed(&p.body))
        .unwrap_or_else(|| {
            std::borrow::Cow::Owned(json!({
        "model":d.profile.model,"messages":[{"role":"user","content":request.prompt}],
        "max_tokens":request.max_tokens,"stream":false}))
        });
    let timeout = crate::input_admission::admit(
        service,
        crate::input_admission::AdmissionInput {
            demand: d,
            invocation,
            body: &body,
        },
    )?;
    let client = client_duration(timeout)?;
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
    client_duration(Duration::from_secs(seconds))
}
fn client_duration(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(timeout)
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

fn ninfer_metrics_idle(metrics: &str) -> bool {
    [
        "llamacpp:requests_processing",
        "llamacpp:requests_deferred",
        "ninfer:requests_admitted",
    ]
    .into_iter()
    .all(|name| {
        let mut samples = metrics.lines().filter_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            (fields.next()? == name).then_some(fields)
        });
        let zero = samples.next().is_some_and(|mut fields| {
            fields.next().and_then(|value| value.parse::<f64>().ok()) == Some(0.0)
                && fields.next().is_none()
        });
        zero && samples.next().is_none()
    })
}

#[cfg(test)]
mod idle_tests {
    use super::ninfer_metrics_idle;

    #[test]
    fn ninfer_idle_requires_explicit_unique_zero_gauges() {
        let idle = "llamacpp:requests_processing 0\nllamacpp:requests_deferred 0\nninfer:requests_admitted 0\n";
        assert!(ninfer_metrics_idle(idle));
        assert!(!ninfer_metrics_idle(
            &idle.replace("deferred 0", "deferred 1")
        ));
        assert!(!ninfer_metrics_idle(
            &idle.replace("admitted 0", "admitted NaN")
        ));
        assert!(!ninfer_metrics_idle(
            &idle.replace("ninfer:requests_admitted 0", "")
        ));
        assert!(!ninfer_metrics_idle(&format!(
            "{idle}llamacpp:requests_processing 1\n"
        )));
    }
}
