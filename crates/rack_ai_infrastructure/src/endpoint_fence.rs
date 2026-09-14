//! Trusted local dispatch barrier. HTTP clients cannot choose this authority root.
use crate::resource_reservations::bounded_lock;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};
#[derive(Deserialize)]
struct Profile {
    endpoint: String,
}
#[derive(Deserialize)]
struct Demand {
    profile: Profile,
    state: State,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum State {
    Denied,
    Preparing,
    Ready,
    Draining,
    Held,
    Releasing,
    Released,
    Cancelled,
    Expired,
    RecoveryRequired,
}
#[derive(Deserialize)]
struct Data {
    demands: BTreeMap<String, Demand>,
    #[serde(default)]
    gateway_port: Option<u16>,
}
#[derive(Deserialize)]
struct Document {
    data: Data,
}
pub struct EndpointFence {
    _guard: Option<File>,
}
impl EndpointFence {
    pub fn local(endpoint: &str) -> Result<Self, String> {
        let root = std::env::var_os("RACK_AI_RESOURCE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/srv/rack-ai/state/resources"));
        Self::enter(&root, endpoint)
    }
    pub fn enter(root: &Path, endpoint: &str) -> Result<Self, String> {
        let port = port(endpoint)?;
        {
            let _authority = bounded_lock(&root.join("authority.lock"))?;
            match fs::read_to_string(root.join("managed.json")) {
                Ok(raw) => {
                    let doc: Document = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                    if doc.data.gateway_port == Some(port) {
                        return Ok(Self { _guard: None });
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.to_string()),
            }
        }
        let guard = bounded_lock(&root.join(format!("endpoint-{port}.lock")))?;
        let _authority = bounded_lock(&root.join("authority.lock"))?;
        match fs::read_to_string(root.join("managed.json")) {
            Ok(raw) => {
                let doc: Document = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                // A port published by this authority is never a legacy bypass, even
                // while its model is temporarily unloaded or its grant is held.
                for d in doc.data.demands.values() {
                    if d.state != State::Denied && self::port(&d.profile.endpoint)? == port {
                        return Err("managed endpoint requires a scoped reservation".into());
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
        Ok(Self {
            _guard: Some(guard),
        })
    }
    pub fn quiescent(root: &Path, endpoint: &str) -> Result<(), String> {
        let port = port(endpoint)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(format!("endpoint-{port}.lock")))
            .map_err(|e| e.to_string())?;
        file.try_lock()
            .map_err(|_| "legacy_dispatch_still_draining".to_string())
    }
}
fn port(endpoint: &str) -> Result<u16, String> {
    endpoint
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| endpoint.strip_prefix("http://[::1]:"))
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| "unqualified local endpoint".into())
}
