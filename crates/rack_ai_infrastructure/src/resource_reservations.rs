use rack_ai_application::{LeaseHandle, durable_file::atomic_write};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const LOCK_TIMEOUT: Duration = Duration::from_secs(3);
const LOCK_POLL: Duration = Duration::from_millis(20);

pub struct ResourceReservations {
    root: PathBuf,
}
pub struct ReservationRequest {
    pub owner: String,
    pub resources: Vec<String>,
    pub acquired_at: String,
    pub worker_ids: Vec<String>,
    pub model_ids: Vec<String>,
}
#[derive(Deserialize, Serialize)]
pub(crate) struct ReservationRecord {
    pub version: u32,
    pub owner: String,
    pub generation: String,
    pub resource_id: String,
    pub task_id: Option<String>,
    pub worker_ids: Vec<String>,
    pub model_ids: Vec<String>,
    pub acquired_at: Option<String>,
}

pub fn new_identity() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn bounded_lock(path: &Path) -> Result<File, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(LOCK_POLL)
            }
            Err(error) => return Err(format!("resource lock unavailable: {error}")),
        }
    }
}
fn valid_resource(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl ResourceReservations {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_resource(id) {
            return Err("invalid resource identity".into());
        }
        Ok(self.root.join("leases").join(format!("{id}.json")))
    }
    pub fn blocked(&self, resources: &[String]) -> Result<Vec<String>, String> {
        let claims = crate::managed_authority::read_claims(&self.root)?;
        let mut blocked = self.legacy_blocked(resources)?;
        blocked.extend(
            resources
                .iter()
                .filter(|id| claims.contains_key(*id))
                .cloned(),
        );
        blocked.sort();
        blocked.dedup();
        Ok(blocked)
    }
    pub fn legacy_blocked(&self, resources: &[String]) -> Result<Vec<String>, String> {
        resources
            .iter()
            .filter_map(|id| match self.path(id) {
                Ok(path) => match fs::symlink_metadata(path) {
                    Ok(_) => Some(Ok(id.clone())),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => Some(Err(e.to_string())),
                },
                Err(e) => Some(Err(e)),
            })
            .collect()
    }
}

impl ResourceReservations {
    pub fn acquire(&self, request: &ReservationRequest) -> Result<LeaseHandle, String> {
        self.acquire_with(request, atomic_write)
    }
    fn acquire_with(
        &self,
        request: &ReservationRequest,
        write: impl Fn(&Path, &str) -> Result<(), String>,
    ) -> Result<LeaseHandle, String> {
        ReservationAcquisition { authority: self }.acquire_with(request, write)
    }
}

impl ResourceReservations {
    pub fn verify(&self, handle: &LeaseHandle) -> Result<(), String> {
        if (crate::managed_lease::ManagedLease { resources: self }).verify(handle, false)? {
            return Ok(());
        }
        for id in handle.paths.keys() {
            let record: ReservationRecord = serde_json::from_str(
                &fs::read_to_string(self.path(id)?).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if record.version != 1
                || record.owner != handle.owner
                || record.generation != handle.generation
                || record.resource_id != *id
            {
                return Err("reservation ownership mismatch".into());
            }
        }
        Ok(())
    }
    pub fn release(&self, handle: &LeaseHandle) -> Result<(), String> {
        if (crate::managed_lease::ManagedLease { resources: self }).request_release(handle)? {
            return Ok(());
        }
        ReservationRelease { authority: self }.execute(handle)
    }
    fn remove_owned(&self, handle: &LeaseHandle) -> Result<(), String> {
        for id in handle.paths.keys() {
            let path = self.path(id)?;
            if !path.try_exists().map_err(|e| e.to_string())? {
                continue;
            }
            let single = LeaseHandle {
                owner: handle.owner.clone(),
                generation: handle.generation.clone(),
                paths: [(id.clone(), String::new())].into_iter().collect(),
            };
            self.verify(&single)?;
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        if !self.root.join("leases").exists() && handle.paths.is_empty() {
            return Ok(());
        }
        File::open(self.root.join("leases"))
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    }
}

struct ReservationAcquisition<'a> {
    authority: &'a ResourceReservations,
}
impl ReservationAcquisition<'_> {
    fn acquire_with(
        &self,
        request: &ReservationRequest,
        write: impl Fn(&Path, &str) -> Result<(), String>,
    ) -> Result<LeaseHandle, String> {
        let _lock = bounded_lock(&self.authority.root.join("authority.lock"))?;
        let unique: std::collections::BTreeSet<_> = request.resources.iter().collect();
        if request.owner.is_empty() || unique.len() != request.resources.len() {
            return Err("invalid reservation request".into());
        }
        if let Some(id) = self.authority.blocked(&request.resources)?.first() {
            return Err(format!("resource busy: {id}"));
        }
        let mut handle = LeaseHandle {
            owner: request.owner.clone(),
            generation: new_identity()?,
            paths: BTreeMap::new(),
        };
        for id in &request.resources {
            let path = self.authority.path(id)?;
            let record = ReservationRecord {
                version: 1,
                owner: handle.owner.clone(),
                generation: handle.generation.clone(),
                resource_id: id.clone(),
                task_id: Some(request.owner.clone()),
                worker_ids: request.worker_ids.clone(),
                model_ids: request.model_ids.clone(),
                acquired_at: Some(request.acquired_at.clone()),
            };
            // Include the attempted path: rename may have succeeded before directory fsync failed.
            handle
                .paths
                .insert(id.clone(), path.to_string_lossy().into_owned());
            if let Err(error) = serde_json::to_string(&record)
                .map_err(|e| e.to_string())
                .and_then(|json| write(&path, &json))
            {
                self.authority
                    .remove_owned(&handle)
                    .map_err(|cleanup| format!("{error}; cleanup uncertain: {cleanup}"))?;
                return Err(error);
            }
        }
        Ok(handle)
    }
}

struct ReservationRelease<'a> {
    authority: &'a ResourceReservations,
}
impl ReservationRelease<'_> {
    fn execute(&self, handle: &LeaseHandle) -> Result<(), String> {
        let _lock = bounded_lock(&self.authority.root.join("authority.lock"))?;
        if handle.paths.is_empty() {
            return Ok(());
        }
        if handle.generation.len() != 32
            || !handle.generation.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("invalid generation".into());
        }
        let receipt = self
            .authority
            .root
            .join("releases")
            .join(format!("{}.json", handle.generation));
        let encoded = serde_json::to_string(handle).map_err(|e| e.to_string())?;
        match fs::read_to_string(&receipt) {
            Ok(saved) if saved == encoded => {
                // A durable release intent permits restart after a partial deletion. A
                // newer owner in ANY remaining record still prevents all deletion.
                let mut remaining = handle.clone();
                remaining.paths.retain(|id, _| {
                    self.authority
                        .path(id)
                        .ok()
                        .is_some_and(|p| fs::symlink_metadata(p).is_ok())
                });
                self.authority.verify(&remaining)?;
            }
            Ok(_) => return Err("release receipt ownership mismatch".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.authority.verify(handle)?;
                atomic_write(&receipt, &encoded)?;
            }
            Err(e) => return Err(e.to_string()),
        }
        self.authority.remove_owned(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("rack-reservation-{}", new_identity().unwrap()))
    }
    fn request(owner: &str) -> ReservationRequest {
        ReservationRequest {
            owner: owner.into(),
            resources: vec!["gpu-fixture".into()],
            acquired_at: "now".into(),
            worker_ids: vec![],
            model_ids: vec![],
        }
    }
    #[test]
    fn simultaneous_independent_instances_have_one_owner() {
        let root = root();
        let barrier = Arc::new(Barrier::new(12));
        let threads: Vec<_> = (0..12)
            .map(|i| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    ResourceReservations::new(root).acquire(&request(&format!("owner-{i}")))
                })
            })
            .collect();
        let handles: Vec<_> = threads
            .into_iter()
            .filter_map(|t| t.join().unwrap().ok())
            .collect();
        assert_eq!(handles.len(), 1);
        let authority = ResourceReservations::new(root);
        authority.verify(&handles[0]).unwrap();
        authority.release(&handles[0]).unwrap();
    }
    #[test]
    fn stale_and_wrong_owner_handles_cannot_release_new_generation() {
        let authority = ResourceReservations::new(root());
        let first = authority.acquire(&request("first")).unwrap();
        let mut wrong = first.clone();
        wrong.owner = "other".into();
        assert!(authority.release(&wrong).is_err());
        authority.release(&first).unwrap();
        let next = authority.acquire(&request("first")).unwrap();
        assert_ne!(first.generation, next.generation);
        assert!(authority.release(&first).is_err());
        authority.verify(&next).unwrap();
    }
    #[test]
    fn legacy_corrupt_symlink_and_crash_remnants_block() {
        for content in ["{}", "{bad", r#"{"task_id":"legacy"}"#] {
            let authority = ResourceReservations::new(root());
            fs::create_dir_all(authority.root.join("leases")).unwrap();
            fs::write(authority.path("gpu-fixture").unwrap(), content).unwrap();
            assert!(authority.acquire(&request("new")).is_err());
            assert_eq!(
                fs::read_to_string(authority.path("gpu-fixture").unwrap()).unwrap(),
                content
            );
        }
        let authority = ResourceReservations::new(root());
        let first = authority.acquire(&request("crash")).unwrap();
        drop(first);
        assert!(authority.acquire(&request("new")).is_err());
    }
    #[test]
    fn partial_invalid_request_does_not_acquire_any_resource() {
        let authority = ResourceReservations::new(root());
        let mut request = request("owner");
        request.resources.push("../escape".into());
        assert!(authority.acquire(&request).is_err());
        assert!(
            authority
                .blocked(&["gpu-fixture".into()])
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn ordinary_second_write_failure_rolls_back_first_record() {
        let authority = ResourceReservations::new(root());
        let mut request = request("owner");
        request.resources.push("gpu-second".into());
        let calls = std::cell::Cell::new(0);
        assert!(
            authority
                .acquire_with(&request, |path, data| {
                    calls.set(calls.get() + 1);
                    if calls.get() == 2 {
                        return Err("injected disk failure".into());
                    }
                    atomic_write(path, data)
                })
                .is_err()
        );
        assert!(authority.blocked(&request.resources).unwrap().is_empty());
    }
    #[test]
    fn release_intent_recovers_after_deletion_before_state_commit() {
        let authority = ResourceReservations::new(root());
        let handle = authority.acquire(&request("owner")).unwrap();
        authority.release(&handle).unwrap();
        authority.release(&handle).unwrap();
        assert!(
            authority
                .blocked(&["gpu-fixture".into()])
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn release_checks_entire_set_before_any_deletion() {
        let authority = ResourceReservations::new(root());
        let mut request = request("owner");
        request.resources.push("gpu-second".into());
        let handle = authority.acquire(&request).unwrap();
        fs::write(authority.path("gpu-second").unwrap(), "{}").unwrap();
        assert!(authority.release(&handle).is_err());
        assert!(authority.path("gpu-fixture").unwrap().exists());
    }
}
