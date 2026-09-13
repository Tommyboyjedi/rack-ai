use crate::types::Priority;
use serde::{Deserialize, Serialize};
use std::{fs, net::SocketAddr, path::PathBuf};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: String,
    pub token_sha256: String,
    pub ceiling: Priority,
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
        let value: Self = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.listen.ip().is_loopback()
            || !self.native_listen.ip().is_loopback()
            || self.listen == self.native_listen
        {
            return Err("distinct loopback listeners required".into());
        }
        for root in [
            &self.state_root,
            &self.resource_root,
            &self.output_root,
            &self.authority_file,
            &self.control_secret_file,
            &self.runtime.python,
            &self.runtime.script,
            &self.runtime.directory,
        ] {
            if !root.is_absolute() {
                return Err("absolute administrator paths required".into());
            }
        }
        let backend = reqwest::Url::parse(&self.backend).map_err(|e| e.to_string())?;
        if backend.scheme() != "http"
            || backend.host_str() != Some("127.0.0.1")
            || backend.path() != "/"
            || backend.query().is_some()
            || !backend.username().is_empty()
        {
            return Err("raw ComfyUI must use loopback".into());
        }
        if self.public_origin == self.native_origin {
            return Err("native UI requires its own origin".into());
        }
        for origin in [&self.public_origin, &self.native_origin] {
            let url = reqwest::Url::parse(origin).map_err(|e| e.to_string())?;
            if url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
                || url.password().is_some()
                || !url.username().is_empty()
                || !(url.scheme() == "https"
                    || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1")))
            {
                return Err("private HTTPS origins (or loopback test origins) required".into());
            }
        }
        if !self.unit.starts_with("rack-ai-comfyui-")
            || !self.unit.ends_with(".service")
            || !self
                .unit
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c))
        {
            return Err("invalid owned candidate unit".into());
        }
        if self.principals.is_empty()
            || self.principals.iter().any(|p| {
                p.id.is_empty()
                    || p.token_sha256.len() != 64
                    || !p.token_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    || (p.id == "athba" && p.ceiling > Priority::Medium)
            })
        {
            return Err("invalid source credentials/ceilings".into());
        }
        let ids: std::collections::BTreeSet<_> = self.principals.iter().map(|p| &p.id).collect();
        let hashes: std::collections::BTreeSet<_> = self
            .principals
            .iter()
            .map(|p| p.token_sha256.to_ascii_lowercase())
            .collect();
        if hashes.len() != self.principals.len() || ids.len() != self.principals.len() {
            return Err("duplicate principal".into());
        }
        if [
            self.start_timeout,
            self.drain_timeout,
            self.stop_timeout,
            self.idle_seconds,
            self.session_seconds,
        ]
        .iter()
        .any(|n| *n == 0 || *n > 86400)
        {
            return Err("invalid lifecycle deadline".into());
        }
        Ok(())
    }
}
