use crate::LeaseStateRepository;
use crate::QueueStateRepository;
use crate::RunStateRepository;
use crate::StatusRun;
use crate::StatusSnapshot;

pub struct InspectStatus<'a> {
    queue_state_repository: &'a dyn QueueStateRepository,
    lease_state_repository: &'a dyn LeaseStateRepository,
    run_state_repository: &'a dyn RunStateRepository,
}

pub struct InspectStatusDependencies<'a> {
    pub queue_state_repository: &'a dyn QueueStateRepository,
    pub lease_state_repository: &'a dyn LeaseStateRepository,
    pub run_state_repository: &'a dyn RunStateRepository,
}

impl<'a> InspectStatus<'a> {
    pub fn new(dependencies: InspectStatusDependencies<'a>) -> Self {
        Self {
            queue_state_repository: dependencies.queue_state_repository,
            lease_state_repository: dependencies.lease_state_repository,
            run_state_repository: dependencies.run_state_repository,
        }
    }

    pub fn execute(&self) -> Result<StatusSnapshot, String> {
        let queued = self.queue_state_repository.queued_entries()?;
        let running = self.queue_state_repository.running_entries()?;
        let leases = self.lease_state_repository.list()?;
        let runs = self.run_state_repository.list()?;
        let mapped = runs.iter().map(StatusRun::from_run_state).collect();
        Ok(StatusSnapshot::new(queued, running, leases, mapped))
    }
}

#[cfg(test)]
mod tests {
    use rack_ai_domain::AttemptLimit;
    use rack_ai_domain::Placement;
    use rack_ai_domain::RunState;
    use rack_ai_domain::RunStateDraft;
    use rack_ai_domain::TaskId;
    use rack_ai_domain::TimeoutSeconds;

    use super::InspectStatus;
    use super::InspectStatusDependencies;
    use crate::LeaseState;
    use crate::LeaseStateRepository;
    use crate::QueueStateRepository;
    use crate::RunStateRepository;

    #[test]
    fn builds_status_snapshot() {
        let inspect = InspectStatus::new(InspectStatusDependencies {
            queue_state_repository: &FakeQueueStateRepository,
            lease_state_repository: &FakeLeaseStateRepository,
            run_state_repository: &FakeRunStateRepository,
        });
        let result = inspect.execute().unwrap();
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("task-9"));
        assert!(json.contains("queued"));
        assert!(json.contains("gpu-2060"));
    }

    struct FakeQueueStateRepository;

    impl QueueStateRepository for FakeQueueStateRepository {
        fn queued_entries(&self) -> Result<Vec<String>, String> {
            Ok(vec!["a.json".to_string()])
        }
        fn running_entries(&self) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
    }

    struct FakeLeaseStateRepository;

    impl LeaseStateRepository for FakeLeaseStateRepository {
        fn list(&self) -> Result<Vec<LeaseState>, String> {
            Ok(vec![LeaseState::new(
                "gpu-2060".to_string(),
                Some("task-9".to_string()),
                vec!["local-coder".to_string()],
                vec!["coder-model".to_string()],
                Some("2026-08-20T20:00:00Z".to_string()),
                "/state/resources/leases/gpu-2060.json".to_string(),
            )])
        }
    }

    struct FakeRunStateRepository;

    impl RunStateRepository for FakeRunStateRepository {
        fn save(&self, _run_state: &RunState) -> Result<(), String> {
            Ok(())
        }
        fn find(&self, _task_id: &TaskId) -> Result<Option<RunState>, String> {
            Ok(None)
        }
        fn list(&self) -> Result<Vec<RunState>, String> {
            Ok(vec![sample_run_state()])
        }
    }

    fn sample_run_state() -> RunState {
        RunState::queued(RunStateDraft {
            task_id: TaskId::new("task-9".to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(2).unwrap(),
            timeout_seconds: TimeoutSeconds::new(60).unwrap(),
            placement: Placement::new(vec!["worker".to_string()], vec!["gpu".to_string()]),
        })
    }
}
