//! Managed claims extend the canonical authority, using the same lock as legacy leases.
use crate::resource_reservations::{ResourceReservations, bounded_lock};
use rack_ai_application::durable_file::atomic_write;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
const MAX_AUTHORITY_BYTES: u64 = 32 * 1024 * 1024;
use std::{collections::BTreeMap, fs, path::Path};
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct AuthorityDocument<T> {
    pub claims: BTreeMap<String, String>,
    pub data: T,
}
#[derive(Deserialize)]
struct ClaimHeader {
    claims: BTreeMap<String, String>,
}
pub fn read_claims(root: &Path) -> Result<BTreeMap<String, String>, String> {
    match fs::read_to_string(root.join("managed.json")) {
        Ok(raw) => serde_json::from_str::<ClaimHeader>(&raw)
            .map(|v| v.claims)
            .map_err(|e| format!("managed authority corrupt: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e.to_string()),
    }
}
pub struct ManagedAuthority<T> {
    pub resources: ResourceReservations,
    marker: std::marker::PhantomData<T>,
}
impl<T: Default + DeserializeOwned + Serialize> ManagedAuthority<T> {
    pub fn new(root: std::path::PathBuf) -> Self {
        Self {
            resources: ResourceReservations::new(root),
            marker: std::marker::PhantomData,
        }
    }
    pub fn update<R>(
        &self,
        action: impl FnOnce(&mut AuthorityDocument<T>) -> Result<R, String>,
    ) -> Result<R, String> {
        let _lock = bounded_lock(&self.resources.root().join("authority.lock"))?;
        let path = self.resources.root().join("managed.json");
        if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_AUTHORITY_BYTES) {
            return Err("managed authority exceeds storage bound".into());
        }
        let mut state = match fs::read_to_string(&path) {
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|e| format!("managed authority corrupt: {e}"))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => AuthorityDocument::default(),
            Err(e) => return Err(e.to_string()),
        };
        let result = action(&mut state)?;
        let encoded = serde_json::to_string(&state).map_err(|e| e.to_string())?;
        if encoded.len() as u64 > MAX_AUTHORITY_BYTES {
            return Err("managed authority retention full".into());
        }
        atomic_write(&path, &encoded)?;
        Ok(result)
    }
}
