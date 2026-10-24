//! Operator-only, read-only view of an independently collected energy snapshot.
use crate::{api::ApiRequest, web_state::WebState};
use axum::http::StatusCode;
use serde::Deserialize;
use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
const MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024;
const MAX_FRESH_AGE: f64 = 90.0;
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnergySource {
    pub snapshot: PathBuf,
}
impl EnergySource {
    pub fn load(root: &Path) -> Result<Option<Self>, String> {
        let path = root.join("energy.json");
        if !path.exists() {
            return Ok(None);
        }
        if fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err("energy source configuration must be private".into());
        }
        let source: Self = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|_| "invalid energy source configuration")?;
        if !source.snapshot.is_absolute() {
            return Err("energy snapshot path must be absolute".into());
        }
        Ok(Some(source))
    }
    fn read(&self) -> Result<serde_json::Value, String> {
        let file = fs::File::open(&self.snapshot).map_err(|_| "energy snapshot unavailable")?;
        let mut bytes = Vec::new();
        file.take(MAX_SNAPSHOT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "energy snapshot unavailable")?;
        if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
            return Err("energy snapshot exceeds bound".into());
        }
        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "invalid energy snapshot")?;
        if value.get("schema").and_then(|v| v.as_str()) != Some("rack-ai/energy/v1") {
            return Err("unsupported energy snapshot".into());
        }
        let at = value
            .get("collected_at")
            .and_then(|v| v.as_f64())
            .ok_or("missing energy timestamp")?;
        let age = crate::types::now() as f64 - at;
        value["stale"] = serde_json::json!(!(0.0..=MAX_FRESH_AGE).contains(&age));
        Ok(value)
    }
}
pub fn handle(
    app: &WebState,
    input: &ApiRequest,
) -> Result<(StatusCode, serde_json::Value), String> {
    if !input.principal.operator {
        return Err("forbidden: operator access required".into());
    }
    let source = app
        .energy
        .as_ref()
        .ok_or("unavailable: energy history is not configured")?;
    let report = source
        .read()
        .map_err(|_| "unavailable: energy history cannot currently be read")?;
    Ok((StatusCode::OK, report))
}
