//! Delegated lifecycle handles are verified against the canonical managed document.
use crate::resource_reservations::ResourceReservations;
use rack_ai_application::{LeaseHandle, durable_file::atomic_write};
use serde::Deserialize;
use std::{collections::BTreeMap, fs};
#[derive(Deserialize)]
struct Profile {
    resources: Vec<String>,
}
#[derive(Deserialize)]
struct Demand {
    owner: String,
    deadline: u64,
    released: bool,
    priority: rack_ai_application::GenericPriority,
    generation: String,
    state: State,
    profile: Profile,
    victims: Vec<String>,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum State {
    Preparing,
    Ready,
    Draining,
    Held,
    Releasing,
    Released,
    Denied,
    Cancelled,
    Expired,
    RecoveryRequired,
}
#[derive(Deserialize)]
struct Data {
    demands: BTreeMap<String, Demand>,
}
#[derive(Deserialize)]
struct Document {
    claims: BTreeMap<String, String>,
    data: Data,
}
pub struct ManagedLease<'a> {
    pub resources: &'a ResourceReservations,
}
impl ManagedLease<'_> {
    pub fn verify(&self, handle: &LeaseHandle, dispatch: bool) -> Result<bool, String> {
        let path = self.resources.root().join("managed.json");
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.to_string()),
        };
        let doc: Document = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let Some(d) = doc.data.demands.get(&handle.owner) else {
            return Ok(false);
        };
        if d.generation != handle.generation
            || handle.paths.len() != d.profile.resources.len()
            || d.profile
                .resources
                .iter()
                .any(|r| !handle.paths.contains_key(r))
        {
            return Err("managed reservation generation/resource mismatch".into());
        }
        for resource in &d.profile.resources {
            let claimant = doc
                .claims
                .get(resource)
                .ok_or("managed resource no longer owned")?;
            let delegated_cleanup = !dispatch
                && d.state == State::Draining
                && doc
                    .data
                    .demands
                    .get(claimant)
                    .is_some_and(|x| x.victims.contains(&handle.owner));
            if claimant != &handle.owner && !delegated_cleanup {
                return Err("managed ownership changed".into());
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        if dispatch && (d.state != State::Ready || d.released || d.deadline <= now) {
            return Err("managed admission closed".into());
        }
        Ok(true)
    }
    pub fn authorize(
        &self,
        handle: &LeaseHandle,
        caller: (&str, rack_ai_application::GenericPriority),
    ) -> Result<(), String> {
        if !self.verify(handle, true)? {
            return Err("managed reservation required".into());
        }
        let raw = fs::read_to_string(self.resources.root().join("managed.json"))
            .map_err(|e| e.to_string())?;
        let doc: Document = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let d = doc
            .data
            .demands
            .get(&handle.owner)
            .ok_or("managed reservation missing")?;
        if d.owner != caller.0 || d.priority != caller.1 {
            return Err("managed caller or priority mismatch".into());
        }
        Ok(())
    }
    pub fn request_release(&self, handle: &LeaseHandle) -> Result<bool, String> {
        let _lock = crate::resource_reservations::bounded_lock(
            &self.resources.root().join("authority.lock"),
        )?;
        if !self.verify(handle, false)? {
            return Ok(false);
        }
        let raw = fs::read_to_string(self.resources.root().join("managed.json"))
            .map_err(|e| e.to_string())?;
        let doc: Document = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        if doc
            .data
            .demands
            .get(&handle.owner)
            .is_some_and(|d| d.state == State::Draining)
        {
            return Ok(true);
        }
        atomic_write(
            &self
                .resources
                .root()
                .join("managed-releases")
                .join(format!("{}.json", handle.generation)),
            &serde_json::to_string(handle).map_err(|e| e.to_string())?,
        )?;
        Ok(true)
    }
}
