use serde::Deserialize;
use serde::Serialize;

use crate::ActiveNodeId;
use crate::DagNodeStatus;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DagNodeState {
    depends_on: Vec<ActiveNodeId>,
    status: DagNodeStatus,
    last_error: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    result_path: Option<String>,
}

impl DagNodeState {
    pub fn pending(depends_on: Vec<ActiveNodeId>) -> Self {
        Self {
            depends_on,
            status: DagNodeStatus::Pending,
            last_error: None,
            started_at: None,
            finished_at: None,
            result_path: None,
        }
    }

    pub fn mark_running(mut self, started_at: String) -> Self {
        self.status = DagNodeStatus::Running;
        self.started_at = Some(started_at);
        self.last_error = None;
        self
    }

    pub fn mark_pending(mut self, last_error: String) -> Self {
        self.status = DagNodeStatus::Pending;
        self.last_error = Some(last_error);
        self
    }

    pub fn mark_succeeded(mut self, finished_at: String, result_path: Option<String>) -> Self {
        self.status = DagNodeStatus::Succeeded;
        self.finished_at = Some(finished_at);
        self.result_path = result_path;
        self.last_error = None;
        self
    }

    pub fn mark_failed(
        mut self,
        finished_at: String,
        result_path: Option<String>,
        last_error: String,
    ) -> Self {
        self.status = DagNodeStatus::Failed;
        self.finished_at = Some(finished_at);
        self.result_path = result_path;
        self.last_error = Some(last_error);
        self
    }

    pub fn depends_on(&self) -> &[ActiveNodeId] {
        self.depends_on.as_slice()
    }
    pub fn status(&self) -> &DagNodeStatus {
        &self.status
    }
}
