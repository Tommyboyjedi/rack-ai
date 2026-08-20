use serde::Serialize;

use crate::StatusRun;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StatusSnapshot {
    queued: Vec<String>,
    running: Vec<String>,
    runs: Vec<StatusRun>,
}

impl StatusSnapshot {
    pub fn new(queued: Vec<String>, running: Vec<String>, runs: Vec<StatusRun>) -> Self {
        Self {
            queued,
            running,
            runs,
        }
    }
}
