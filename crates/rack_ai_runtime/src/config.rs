use crate::types::*;
use rack_ai_application::GenericCapability;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Vllm,
    LlamaCpp,
    Comfyui,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Driver {
    Systemd,
    Docker,
    Fixture,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub tag: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    pub backend: Backend,
    pub driver: Driver,
    pub qualified: bool,
    pub evidence: Vec<String>,
    pub capabilities: Vec<GenericCapability>,
    pub context_tokens: u32,
    #[serde(default = "crate::protocol::default_protocols")]
    pub protocols: Vec<crate::protocol::Protocol>,
    #[serde(default)]
    pub streaming: bool,
    pub max_output_tokens: u32,
    pub resources: Vec<String>,
    pub device_mib: BTreeMap<String, u64>,
    pub host_mib: u64,
    pub cpu_percent: u32,
    pub endpoint: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub container_image: Option<String>,
    #[serde(default)]
    pub container_mounts: BTreeMap<PathBuf, String>,
    pub executable_sha256: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub media_config: Option<PathBuf>,
    #[serde(default)]
    pub media_config_sha256: Option<String>,
    #[serde(default)]
    pub media_mode: Option<rack_ai_media::types::Mode>,
    pub artifact: Option<PathBuf>,
    pub artifact_sha256: Option<String>,
    #[serde(default = "artifact_timeout")]
    pub artifact_verify_seconds: u64,
    pub startup_seconds: u64,
    pub drain_seconds: u64,
    pub stop_seconds: u64,
    pub inference_seconds: u64,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub source: String,
    pub token_sha256: String,
    #[serde(default)]
    pub permitted: Vec<Priority>,
    #[serde(default = "default_priority")]
    pub default: Priority,
    #[serde(default = "default_priority")]
    pub maximum: Priority,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tag_priorities: BTreeMap<String, Vec<Priority>>,
    #[serde(default)]
    pub qualification: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub uuid: String,
    pub capacity_mib: u64,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub workspace: Option<crate::work_payload::WorkspaceConfig>,
    pub schema: String,
    pub listen: String,
    pub authority_root: PathBuf,
    pub fixture_mode: bool,
    pub max_ttl_seconds: u64,
    #[serde(default = "crate::idle::default_seconds")]
    pub idle_timeout_seconds: u64,
    #[serde(default)]
    pub limits: crate::capacity::Limits,
    pub devices: BTreeMap<String, Device>,
    pub host_capacity_mib: u64,
    pub sources: Vec<Source>,
    pub profiles: Vec<Profile>,
}
impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err("administrator configuration must be mode 0600".into());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let config: Self = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        crate::validation::validate(&config)?;
        Ok(config)
    }
    pub fn authenticate(&self, token: &str) -> Result<Source, String> {
        use subtle::ConstantTimeEq;
        let hash = digest(token.as_bytes());
        self.sources
            .iter()
            .find(|s| bool::from(s.token_sha256.as_bytes().ct_eq(hash.as_bytes())))
            .cloned()
            .ok_or_else(|| "unauthorized".into())
    }
}

fn artifact_timeout() -> u64 {
    900
}

impl Profile {
    pub fn native_media(&self) -> bool {
        self.backend == Backend::Comfyui
            && self
                .media_mode
                .unwrap_or(rack_ai_media::types::Mode::Interactive)
                == rack_ai_media::types::Mode::Interactive
    }
}

// Legacy source policy fields deserialize only for migration.
fn default_priority() -> Priority {
    Priority::Low
}
