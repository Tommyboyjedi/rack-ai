use crate::types::Demand;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Deserialize)]
pub(super) struct EngineConfig {
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl EngineConfig {
    pub fn load(demand: &Demand, status: &Value) -> Result<Self, String> {
        let configs: Vec<_> = demand
            .profile
            .args
            .windows(2)
            .filter(|v| v[0] == "--config")
            .collect();
        if configs.len() != 1 {
            return Err("strata_config_argument_ambiguous".into());
        }
        let path = PathBuf::from(&configs[0][1]);
        let launch = demand
            .profile
            .launch
            .as_ref()
            .ok_or("strata_launch_missing")?;
        let pin = launch
            .file_pins
            .iter()
            .find(|pin| pin.path == path)
            .ok_or("strata_config_pin_missing")?;
        let bytes = fs::read(&path).map_err(|_| "strata_config_unreadable")?;
        let hash = crate::types::digest(&bytes);
        if hash != pin.sha256 || status["config_sha256"].as_str() != Some(hash.as_str()) {
            return Err("strata_config_identity_mismatch".into());
        }
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| "strata_config_invalid")?;
        if !launch.file_pins.iter().any(|pin| pin.path == config.exe) {
            return Err("strata_engine_pin_missing".into());
        }
        Ok(config)
    }
}
