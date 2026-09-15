//! Managed claims extend the canonical authority, using the same lock as legacy leases.
use crate::resource_reservations::{ResourceReservations, bounded_lock};
use rack_ai_application::durable_file::atomic_write;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
pub const MAX_AUTHORITY_BYTES: u64 = 32 * 1024 * 1024;
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
    pub fn read<R>(
        &self,
        action: impl FnOnce(&AuthorityDocument<T>) -> Result<R, String>,
    ) -> Result<R, String> {
        let (state, _) = self.load()?;
        action(&state)
    }
    fn load(&self) -> Result<(AuthorityDocument<T>, Vec<u8>), String> {
        use std::io::Read;
        let path = self.resources.root().join("managed.json");
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((AuthorityDocument::default(), Vec::new()));
            }
            Err(e) => return Err(e.to_string()),
        };
        let mut raw = Vec::new();
        file.take(MAX_AUTHORITY_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|e| e.to_string())?;
        if raw.len() as u64 > MAX_AUTHORITY_BYTES {
            return Err("managed authority exceeds storage bound".into());
        }
        let state =
            serde_json::from_slice(&raw).map_err(|e| format!("managed authority corrupt: {e}"))?;
        Ok((state, raw))
    }
    pub fn update<R>(
        &self,
        action: impl FnOnce(&mut AuthorityDocument<T>) -> Result<R, String>,
    ) -> Result<R, String> {
        let _lock = bounded_lock(&self.resources.root().join("authority.lock"))?;
        let path = self.resources.root().join("managed.json");
        let (mut state, before) = self.load()?;
        let result = action(&mut state)?;
        let encoded = serde_json::to_string(&state).map_err(|e| e.to_string())?;
        if encoded.len() as u64 > MAX_AUTHORITY_BYTES {
            return Err("managed authority retention full".into());
        }
        if encoded.as_bytes() != before {
            atomic_write(&path, &encoded)?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    #[test]
    fn read_only_snapshot_and_hard_retention_failure_preserve_evidence() {
        let root = std::env::temp_dir().join(format!(
            "rack-authority-bound-{}",
            crate::resource_reservations::new_identity().unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        let authority = ManagedAuthority::<String>::new(root.clone());
        authority
            .update(|s| {
                s.data = "x".repeat(MAX_AUTHORITY_BYTES as usize - 65536);
                Ok(())
            })
            .unwrap();
        let path = root.join("managed.json");
        let inode = fs::metadata(&path).unwrap().ino();
        let length = authority.read(|s| Ok(s.data.len())).unwrap();
        authority
            .read(|s| {
                assert_eq!(s.data.len(), length);
                Ok(())
            })
            .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert!(
            authority
                .update(|s| {
                    s.data.push_str(&"y".repeat(65536));
                    Ok(())
                })
                .unwrap_err()
                .contains("retention full")
        );
        assert_eq!(authority.read(|s| Ok(s.data.len())).unwrap(), length);
        // A bounded cleanup transition still fits without deleting the retained evidence.
        authority
            .update(|s| {
                s.data.push_str(" terminal cleanup");
                Ok(())
            })
            .unwrap();
        assert!(
            authority
                .read(|s| Ok(s.data.starts_with(&"x".repeat(length))))
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
