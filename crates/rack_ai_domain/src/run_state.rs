use serde::Deserialize;
use serde::Serialize;

use crate::ActiveNodeId;
use crate::AttemptCount;
use crate::AttemptLimit;
use crate::DagRunState;
use crate::Placement;
use crate::RunMetadata;
use crate::RunStatus;
use crate::TaskId;
use crate::TimeoutSeconds;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunState {
    task_id: TaskId,
    status: RunStatus,
    #[serde(rename = "attempt")]
    attempt_count: AttemptCount,
    #[serde(rename = "max_attempts")]
    attempt_limit: AttemptLimit,
    timeout_seconds: TimeoutSeconds,
    placement: Placement,
    active_node_id: Option<ActiveNodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dag_run_state: Option<DagRunState>,
    #[serde(flatten)]
    metadata: RunMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunStateDraft {
    pub task_id: TaskId,
    pub attempt_limit: AttemptLimit,
    pub timeout_seconds: TimeoutSeconds,
    pub placement: Placement,
}

impl RunState {
    pub fn queued(draft: RunStateDraft) -> Self {
        Self {
            task_id: draft.task_id,
            status: RunStatus::Queued,
            attempt_count: AttemptCount::zero(),
            attempt_limit: draft.attempt_limit,
            timeout_seconds: draft.timeout_seconds,
            placement: draft.placement,
            active_node_id: None,
            dag_run_state: None,
            metadata: RunMetadata::default(),
        }
    }

    pub fn start(mut self, active_node_id: Option<ActiveNodeId>) -> Self {
        self.status = RunStatus::Running;
        self.attempt_count = self.attempt_count.incremented();
        self.active_node_id = active_node_id;
        self
    }

    pub fn queue(mut self) -> Self {
        self.status = RunStatus::Queued;
        self.active_node_id = None;
        self
    }

    pub fn succeed(mut self) -> Self {
        self.status = RunStatus::Succeeded;
        self.active_node_id = None;
        self
    }

    pub fn fail(mut self) -> Self {
        self.status = RunStatus::Failed;
        self.active_node_id = None;
        self
    }

    pub fn with_dag_run_state(mut self, dag_run_state: DagRunState) -> Self {
        self.dag_run_state = Some(dag_run_state);
        self
    }

    pub fn with_metadata(mut self, metadata: RunMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn can_retry(&self) -> bool {
        self.attempt_count.value() < self.attempt_limit.value()
    }

    pub fn status(&self) -> &RunStatus {
        &self.status
    }
    pub fn attempt_count(&self) -> AttemptCount {
        self.attempt_count
    }
    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }
    pub fn attempt_limit(&self) -> AttemptLimit {
        self.attempt_limit
    }
    pub fn timeout_seconds(&self) -> TimeoutSeconds {
        self.timeout_seconds
    }
    pub fn placement(&self) -> &Placement {
        &self.placement
    }
    pub fn active_node_id(&self) -> Option<&ActiveNodeId> {
        self.active_node_id.as_ref()
    }
    pub fn dag_run_state(&self) -> Option<&DagRunState> {
        self.dag_run_state.as_ref()
    }
    pub fn metadata(&self) -> &RunMetadata {
        &self.metadata
    }
}
