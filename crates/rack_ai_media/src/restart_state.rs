use crate::backend_generation::BackendGeneration;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RestartPhase {
    Draining,
    Stopping,
    StartPending,
    Starting,
    Finishing,
    Completed,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RestartIntent {
    pub id: String,
    pub phase: RestartPhase,
    pub since: u64,
    pub previous: BackendGeneration,
    pub target: Option<BackendGeneration>,
}
