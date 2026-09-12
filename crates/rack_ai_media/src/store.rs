use crate::types::{MediaState, VERSION};
use rack_ai_application::durable_file::atomic_write;
use rack_ai_infrastructure::resource_reservations::bounded_lock;
use std::{fs, path::PathBuf};
const MAX_STATE_BYTES: u64 = 32 * 1024 * 1024;
#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn initialize(&self) -> Result<(), String> {
        let _lock = bounded_lock(&self.root.join("state.lock"))?;
        let path = self.root.join("state.json");
        if !path.try_exists().map_err(|e| e.to_string())? {
            atomic_write(
                &path,
                &serde_json::to_string(&MediaState::default()).map_err(|e| e.to_string())?,
            )?;
        }
        self.read().map(|_| ())
    }
    pub fn read(&self) -> Result<MediaState, String> {
        let path = self.root.join("state.json");
        if fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .len()
            > MAX_STATE_BYTES
        {
            return Err("state exceeds bound".into());
        }
        let state: MediaState = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("state corrupt: {e}"))?;
        if state.schema != VERSION {
            return Err("incompatible media state".into());
        }
        Ok(state)
    }
    pub fn update<T>(
        &self,
        action: impl FnOnce(&mut MediaState) -> Result<T, String>,
    ) -> Result<T, String> {
        let _lock = bounded_lock(&self.root.join("state.lock"))?;
        let mut state = self.read()?;
        let value = action(&mut state)?;
        let text = serde_json::to_string(&state).map_err(|e| e.to_string())?;
        if text.len() as u64 > MAX_STATE_BYTES {
            return Err("state capacity exceeded".into());
        }
        atomic_write(&self.root.join("state.json"), &text)?;
        Ok(value)
    }
}
