use serde::Deserialize;
use serde::Serialize;

use crate::ActiveNodeId;
use crate::DagNodeStatus;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DagNodeState {
    depends_on: Vec<ActiveNodeId>,
    status: DagNodeStatus,
    last_error: Option<String>,
}

impl DagNodeState {
    pub fn pending(depends_on: Vec<ActiveNodeId>) -> Self {
        Self {
            depends_on,
            status: DagNodeStatus::Pending,
            last_error: None,
        }
    }

    pub fn mark_running(mut self) -> Self {
        self.status = DagNodeStatus::Running;
        self.last_error = None;
        self
    }

    pub fn mark_pending(mut self, last_error: String) -> Self {
        self.status = DagNodeStatus::Pending;
        self.last_error = Some(last_error);
        self
    }

    pub fn mark_succeeded(mut self) -> Self {
        self.status = DagNodeStatus::Succeeded;
        self.last_error = None;
        self
    }

    pub fn mark_failed(mut self, last_error: String) -> Self {
        self.status = DagNodeStatus::Failed;
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

#[cfg(test)]
mod tests {
    use super::DagNodeState;
    use crate::ActiveNodeId;
    use crate::DagNodeStatus;

    #[test]
    fn transitions_between_pending_running_and_succeeded() {
        let state = DagNodeState::pending(vec![sample_node_id()]);
        assert_eq!(state.status(), &DagNodeStatus::Pending);
        let running = state.mark_running();
        let succeeded = running.mark_succeeded();
        assert_eq!(succeeded.status(), &DagNodeStatus::Succeeded);
    }

    #[test]
    fn stores_last_error_when_requeued_or_failed() {
        let pending = DagNodeState::pending(vec![]).mark_pending("boom".to_string());
        let failed = DagNodeState::pending(vec![]).mark_failed("boom".to_string());
        assert_eq!(pending.status(), &DagNodeStatus::Pending);
        assert_eq!(failed.status(), &DagNodeStatus::Failed);
    }

    fn sample_node_id() -> ActiveNodeId {
        ActiveNodeId::new("plan".to_string()).unwrap()
    }
}
