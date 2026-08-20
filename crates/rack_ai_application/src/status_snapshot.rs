use serde::Serialize;

use crate::LeaseState;
use crate::StatusRun;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StatusSnapshot {
    queued: Vec<String>,
    running: Vec<String>,
    leases: Vec<LeaseState>,
    runs: Vec<StatusRun>,
}

impl StatusSnapshot {
    pub fn new(
        queued: Vec<String>,
        running: Vec<String>,
        leases: Vec<LeaseState>,
        runs: Vec<StatusRun>,
    ) -> Self {
        Self {
            queued,
            running,
            leases,
            runs,
        }
    }

    pub fn queued(&self) -> &[String] {
        self.queued.as_slice()
    }

    pub fn running(&self) -> &[String] {
        self.running.as_slice()
    }

    pub fn leases(&self) -> &[LeaseState] {
        self.leases.as_slice()
    }

    pub fn runs(&self) -> &[StatusRun] {
        self.runs.as_slice()
    }
}
