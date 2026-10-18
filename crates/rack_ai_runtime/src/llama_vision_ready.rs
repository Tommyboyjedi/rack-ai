//! Verify opt-in native multimodal llama.cpp activations are loaded.
use crate::types::Demand;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;
use std::{io::Read, time::Duration};
const MAX_PROPS_BYTES: u64 = 65536;

#[derive(Deserialize)]
struct Props {
    modalities: Modalities,
    default_generation_settings: Generation,
    total_slots: u32,
    is_sleeping: bool,
}
#[derive(Deserialize)]
struct Modalities {
    vision: bool,
}
#[derive(Deserialize)]
struct Generation {
    n_ctx: u32,
}

pub fn ready(demand: &Demand) -> Result<(), String> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| "llama_vision_readiness_client")?;
    let response = client
        .get(format!(
            "{}/props",
            demand.profile.endpoint.trim_end_matches('/')
        ))
        .send()
        .map_err(|_| "llama_vision_readiness_transport")?;
    if !response.status().is_success() {
        return Err("llama_vision_readiness_http".into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_PROPS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "llama_vision_readiness_read")?;
    if bytes.len() as u64 > MAX_PROPS_BYTES {
        return Err("llama_vision_readiness_oversized".into());
    }
    let body = serde_json::from_slice(&bytes).map_err(|_| "llama_vision_readiness_invalid_json")?;
    loaded(body, demand.profile.context_tokens)
}

fn loaded(body: Value, context_tokens: u32) -> Result<(), String> {
    let props: Props =
        serde_json::from_value(body).map_err(|_| "llama_vision_readiness_invalid_properties")?;
    if !props.modalities.vision || props.is_sleeping {
        return Err("llama_vision_not_loaded".into());
    }
    if props.default_generation_settings.n_ctx != context_tokens || props.total_slots != 1 {
        return Err("llama_vision_context_mismatch".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
