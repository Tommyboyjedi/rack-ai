use rack_ai_domain::{Placement, TaskId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LeaseHandle {
    pub owner: String,
    pub generation: String,
    pub paths: BTreeMap<String, String>,
}

pub struct LeaseRequest<'a> {
    pub task_id: &'a TaskId,
    pub placement: &'a Placement,
    pub acquired_at: &'a str,
}

pub trait LeaseRepository {
    fn blocked_resources(&self, placement: &Placement) -> Result<Vec<String>, String>;
    fn acquire(&self, request: &LeaseRequest<'_>) -> Result<LeaseHandle, String>;
    fn renew(&self, handle: &LeaseHandle) -> Result<(), String>;
    fn release(&self, handle: &LeaseHandle) -> Result<(), String>;
}
