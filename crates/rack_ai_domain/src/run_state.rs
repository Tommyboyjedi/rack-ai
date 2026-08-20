use serde::Deserialize;
use serde::Serialize;

use crate::ActiveNodeId;
use crate::AttemptCount;
use crate::AttemptLimit;
use crate::Placement;
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
}

#[cfg(test)]
mod tests {
    use super::RunState;
    use super::RunStateDraft;
    use crate::ActiveNodeId;
    use crate::AttemptLimit;
    use crate::Placement;
    use crate::RunStatus;
    use crate::TaskId;
    use crate::TimeoutSeconds;

    #[test]
    fn creates_queued_run_state() {
        let run_state = RunState::queued(sample_draft());
        assert_eq!(run_state.status(), &RunStatus::Queued);
        assert_eq!(run_state.attempt_count().value(), 0);
        assert_eq!(run_state.task_id().value(), "task-1");
    }

    #[test]
    fn starts_run_state() {
        let run_state = RunState::queued(sample_draft()).start(Some(sample_node_id()));
        assert_eq!(run_state.status(), &RunStatus::Running);
        assert_eq!(run_state.attempt_count().value(), 1);
    }

    #[test]
    fn supports_success_failure_and_requeue_states() {
        let queued = RunState::queued(sample_draft());
        assert!(queued.can_retry());
        let running = queued.start(None);
        let requeued = running.clone().queue();
        let failed = running.clone().fail();
        let succeeded = running.succeed();
        assert_eq!(requeued.status(), &RunStatus::Queued);
        assert_eq!(failed.status(), &RunStatus::Failed);
        assert_eq!(succeeded.status(), &RunStatus::Succeeded);
    }

    fn sample_draft() -> RunStateDraft {
        RunStateDraft {
            task_id: TaskId::new("task-1".to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(2).unwrap(),
            timeout_seconds: TimeoutSeconds::new(120).unwrap(),
            placement: Placement::new(vec!["worker-a".to_string()], vec!["gpu-a".to_string()]),
        }
    }

    fn sample_node_id() -> ActiveNodeId {
        ActiveNodeId::new("plan".to_string()).unwrap()
    }
}
