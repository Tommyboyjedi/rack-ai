use serde::{Deserialize, Serialize};
use std::{fs, net::SocketAddr, path::PathBuf};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: String,
    pub token_sha256: String,
    pub operator: bool,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub native_listen: SocketAddr,
    pub public_origin: String,
    pub native_origin: String,
    pub state_root: PathBuf,
    #[serde(default)]
    pub browser_auth_file: Option<PathBuf>,
    pub resource_root: PathBuf,
    pub output_root: PathBuf,
    pub authority_file: PathBuf,
    pub control_secret_file: PathBuf,
    pub unit: String,
    #[serde(default)]
    pub runtime: crate::process_identity::RuntimePaths,
    pub backend: String,
    pub media_uuid: String,
    pub protected: Vec<ProtectedWorker>,
    pub profile: Profile,
    pub start_timeout: u64,
    pub drain_timeout: u64,
    pub stop_timeout: u64,
    pub idle_seconds: u64,
    #[serde(default = "crate::idle::default_seconds")]
    pub reservation_idle_seconds: u64,
    pub session_seconds: u64,
    pub min_memory_mb: u64,
    pub min_disk_mb: u64,
    pub principals: Vec<Principal>,
}
#[derive(Clone, Deserialize)]
pub struct ProtectedWorker {
    pub container: String,
    pub uuid: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub version: u32,
    pub checkpoint: PathBuf,
    pub checkpoint_sha256: String,
    pub runtime_revision: String,
    pub available: bool,
}
impl Config {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err("configuration must be mode 0600".into());
        }
        let mut raw: serde_json::Value =
            serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        // Pre-generic pinned deployments retain this obsolete field. It grants
        // no authority and imposes no priority policy; preserve the original file.
        if let Some(principals) = raw
            .get_mut("principals")
            .and_then(serde_json::Value::as_array_mut)
        {
            for principal in principals {
                if let Some(fields) = principal.as_object_mut() {
                    fields.remove("ceiling");
                }
            }
        }
        let value: Self = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        crate::config_policy::validate(self)
    }
}
