//! Verify the prepared Strata server, its actual engine and loaded context.
mod owned;
mod vision;
use crate::{
    config::{Config, Driver},
    process,
    types::Demand,
};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{io::Read, time::Duration};
const MAX_STATUS_BYTES: u64 = 65536;

pub fn ready(config: &Config, demand: &Demand) -> Result<(), String> {
    let parent = demand
        .process
        .as_ref()
        .ok_or("activation_process_missing")?;
    process::endpoint_owned(parent, &demand.profile)?;
    let client = status_client()?;
    let status = read(&client, (&demand.profile.endpoint, "/rackai/status"))?;
    if status["schema"] != "rack-ai/strata-runtime/v1"
        || status["service"] != "strata"
        || status["model"].as_str() != Some(demand.profile.model.as_str())
        || status["max_context"].as_u64() != Some(demand.profile.context_tokens as u64)
        || status["loaded"] != true
        || status["structured_output"]["method"] != "prompt_and_validate"
        || status["structured_output"]["constrained_decoding"] != false
    {
        return Err("strata_runtime_identity_mismatch".into());
    }
    let child = if demand.profile.driver == Driver::Fixture {
        None
    } else {
        crate::launch_identity::devices(config, demand)?;
        Some(owned::verify(demand, &status)?)
    };
    let encoder = if demand.profile.driver == Driver::Fixture {
        if demand.profile.max_images_per_request > 0
            && (status["vision"]["enabled"] != true
                || status["vision"]["loaded"] != true
                || status["vision"]["protocol"] != "rackai-count-v1")
        {
            return Err("strata_vision_readiness_mismatch".into());
        }
        None
    } else {
        vision::verify(demand, &status)?
    };
    let health = read(&client, (&demand.profile.endpoint, "/health"))?;
    if health["status"] != "ok"
        || health["service"] != "strata"
        || health["loaded"] != true
        || health["model"].as_str() != Some(demand.profile.model.as_str())
        || health["max_context"].as_u64() != Some(demand.profile.context_tokens as u64)
    {
        return Err("strata_loaded_model_context_mismatch".into());
    }
    process::endpoint_owned(parent, &demand.profile)?;
    for child in child.into_iter().chain(encoder) {
        let current = process::capture(child.pid, &child.activation)?;
        if current.boot != child.boot || current.start != child.start {
            return Err("strata_engine_generation_changed".into());
        }
    }
    Ok(())
}

fn read(client: &Client, address: (&str, &str)) -> Result<Value, String> {
    let response = client
        .get(format!("{}{}", address.0.trim_end_matches('/'), address.1))
        .send()
        .map_err(|_| "strata_readiness_transport")?;
    if !response.status().is_success() {
        return Err("strata_readiness_http".into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_STATUS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "strata_readiness_read")?;
    if bytes.len() as u64 > MAX_STATUS_BYTES {
        return Err("strata_readiness_oversized".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "strata_readiness_invalid_json".into())
}

fn status_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| "strata_readiness_client".into())
}

pub(crate) fn idle(config: &Config, demand: &Demand) -> Result<bool, String> {
    let _guard = rack_ai_infrastructure::endpoint_fence::managed_read_guard(
        &config.authority_root,
        &demand.profile.endpoint,
    )?;
    ready(config, demand)?;
    let client = status_client()?;
    let slots = read(&client, (&demand.profile.endpoint, "/slots"))?;
    let status = read(&client, (&demand.profile.endpoint, "/v1/status"))?;
    let parent = demand
        .process
        .as_ref()
        .ok_or("activation_process_missing")?;
    process::endpoint_owned(parent, &demand.profile)?;
    Ok(engine_idle(&slots, &status))
}

fn engine_idle(slots: &Value, status: &Value) -> bool {
    status["loaded"] == true
        && status["concurrency"]["serving"].as_u64() == Some(1)
        && status["activity"]["in_flight"].as_u64() == Some(0)
        && slots.as_array().is_some_and(
            |slots| matches!(slots.as_slice(), [slot] if slot["is_processing"] == false),
        )
}

#[cfg(test)]
mod idle_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_an_explicit_idle_single_slot_is_proof() {
        let idle = json!({"loaded":true, "concurrency":{"serving":1}, "activity":{"in_flight":0}});
        assert!(engine_idle(&json!([{"is_processing":false}]), &idle));
        for slots in [json!([]), json!([{}]), json!([{"is_processing":true}])] {
            assert!(!engine_idle(&slots, &idle));
        }
        assert!(!engine_idle(&json!([{"is_processing":false}]), &json!({})));
    }
}
