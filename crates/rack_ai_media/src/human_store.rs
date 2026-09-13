use crate::{config::Config, password_kdf, types::identity};
use rack_ai_application::durable_file::atomic_write_private;
use rack_ai_infrastructure::resource_reservations::bounded_lock;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::PathBuf,
};
const RECORD_BYTES: u64 = 4096;
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HumanRecord {
    pub generation: String,
    pub owner: Option<String>,
    pub password_hash: Option<String>,
}
#[derive(Clone)]
pub struct HumanStore {
    path: PathBuf,
}
impl HumanStore {
    pub fn new(config: &Config) -> Result<Self, String> {
        let path = config
            .browser_auth_file
            .clone()
            .unwrap_or_else(|| config.state_root.join("browser-auth/auth.json"));
        let parent = path
            .parent()
            .ok_or("Browser authentication directory missing")?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|_| "Browser authentication directory unavailable")?;
        let metadata = fs::symlink_metadata(parent)
            .map_err(|_| "Browser authentication directory unavailable")?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err("Browser authentication directory must be private mode 0700".into());
        }
        Ok(Self { path })
    }
    pub fn lock(&self) -> Result<fs::File, String> {
        bounded_lock(&self.path.with_extension("lock"))
            .map_err(|_| "Browser authentication busy".into())
    }
    pub fn read(&self) -> Result<HumanRecord, String> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HumanRecord::default()),
            Err(_) => return Err("Browser authentication unavailable".into()),
        };
        let parent = self
            .path
            .parent()
            .ok_or("Browser authentication directory missing")?;
        let owner = fs::metadata(parent)
            .map_err(|_| "Browser authentication unavailable")?
            .uid();
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o777 != 0o600
            || metadata.len() > RECORD_BYTES
            || metadata.uid() != owner
        {
            return Err("Browser authentication record is not a private regular file".into());
        }
        let record: HumanRecord = serde_json::from_slice(
            &fs::read(&self.path).map_err(|_| "Browser authentication unavailable")?,
        )
        .map_err(|_| "Browser authentication record is invalid")?;
        if record.password_hash.is_some() != record.owner.is_some()
            || (record.password_hash.is_some() && record.generation.is_empty())
        {
            return Err("Browser authentication record is invalid".into());
        }
        if let Some(hash) = &record.password_hash {
            password_kdf::validate_hash(hash)?;
        }
        Ok(record)
    }
    pub fn write(&self, record: &HumanRecord) -> Result<(), String> {
        let text =
            serde_json::to_string(record).map_err(|_| "Browser authentication unavailable")?;
        atomic_write_private(&self.path, &text)
            .map_err(|_| "Browser authentication write failed".into())
    }
    pub fn reset(&self) -> Result<(), String> {
        // Shell-only operation: rotation revokes every old cookie without touching media/lease state.
        let _lock = self.lock()?;
        self.write(&HumanRecord {
            generation: identity(),
            owner: None,
            password_hash: None,
        })
    }
}
