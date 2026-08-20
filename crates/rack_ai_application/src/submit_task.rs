use rack_ai_domain::DagRunState;
use rack_ai_domain::RunMetadata;
use rack_ai_domain::RunState;
use rack_ai_domain::RunStateDraft;

use crate::RunStateRepository;
use crate::TaskSpecRepository;

pub struct SubmitTask<'a> {
    run_state_repository: &'a dyn RunStateRepository,
    task_spec_repository: &'a dyn TaskSpecRepository,
}

pub struct SubmitTaskDependencies<'a> {
    pub run_state_repository: &'a dyn RunStateRepository,
    pub task_spec_repository: &'a dyn TaskSpecRepository,
}

pub struct SubmitTaskRequest {
    pub spec_json: String,
    pub run_state: RunStateDraft,
    pub dag_run_state: Option<DagRunState>,
    pub submitted_at: String,
    pub source_spec: String,
    pub queue_path: String,
}

impl<'a> SubmitTask<'a> {
    pub fn new(repositories: SubmitTaskDependencies<'a>) -> Self {
        Self {
            run_state_repository: repositories.run_state_repository,
            task_spec_repository: repositories.task_spec_repository,
        }
    }

    pub fn execute(&self, request: SubmitTaskRequest) -> Result<RunState, String> {
        let run_state =
            RunState::queued(request.run_state).with_metadata(RunMetadata::default().submitted(
                request.submitted_at,
                request.source_spec,
                request.queue_path,
            ));
        let run_state = if let Some(dag_run_state) = request.dag_run_state {
            run_state.with_dag_run_state(dag_run_state)
        } else {
            run_state
        };
        self.task_spec_repository
            .save(run_state.task_id().value(), request.spec_json.as_str())?;
        self.run_state_repository.save(&run_state)?;
        Ok(run_state)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use rack_ai_domain::AttemptLimit;
    use rack_ai_domain::Placement;
    use rack_ai_domain::RunState;
    use rack_ai_domain::RunStateDraft;
    use rack_ai_domain::TaskId;
    use rack_ai_domain::TimeoutSeconds;

    use super::SubmitTask;
    use super::SubmitTaskDependencies;
    use super::SubmitTaskRequest;
    use crate::QueuedTask;
    use crate::RunStateRepository;
    use crate::TaskSpec;
    use crate::TaskSpecRepository;

    #[test]
    fn saves_spec_and_submitted_run_metadata() {
        let run_state_repository = FakeRunStateRepository::default();
        let task_spec_repository = FakeTaskSpecRepository::default();
        let service = SubmitTask::new(SubmitTaskDependencies {
            run_state_repository: &run_state_repository,
            task_spec_repository: &task_spec_repository,
        });

        let result = service
            .execute(SubmitTaskRequest {
                spec_json: "{\"task_id\":\"task-22\"}".to_string(),
                run_state: sample_run_state_draft(),
                dag_run_state: None,
                submitted_at: "2026-08-20T20:30:00Z".to_string(),
                source_spec: "/tmp/spec.json".to_string(),
                queue_path: "/state/queue/queued/task-22.json".to_string(),
            })
            .unwrap();

        assert_eq!(
            result.metadata().submitted_at(),
            Some(&"2026-08-20T20:30:00Z".to_string())
        );
        assert_eq!(
            result.metadata().source_spec(),
            Some(&"/tmp/spec.json".to_string())
        );
        assert_eq!(
            result.metadata().queue_path(),
            Some(&"/state/queue/queued/task-22.json".to_string())
        );
        assert_eq!(
            result.metadata().admission_state(),
            Some(&"queued".to_string())
        );
        assert_eq!(
            task_spec_repository.saved_task_id.borrow().as_deref(),
            Some("task-22")
        );
        assert_eq!(
            task_spec_repository.saved_json.borrow().as_deref(),
            Some("{\"task_id\":\"task-22\"}")
        );
        assert_eq!(run_state_repository.saved.borrow().len(), 1);
    }

    #[derive(Default)]
    struct FakeRunStateRepository {
        saved: RefCell<Vec<RunState>>,
    }

    impl RunStateRepository for FakeRunStateRepository {
        fn save(&self, run_state: &RunState) -> Result<(), String> {
            self.saved.borrow_mut().push(run_state.clone());
            Ok(())
        }

        fn find(&self, _task_id: &TaskId) -> Result<Option<RunState>, String> {
            Ok(None)
        }

        fn list(&self) -> Result<Vec<RunState>, String> {
            Ok(vec![])
        }
    }

    #[derive(Default)]
    struct FakeTaskSpecRepository {
        saved_task_id: RefCell<Option<String>>,
        saved_json: RefCell<Option<String>>,
    }

    impl TaskSpecRepository for FakeTaskSpecRepository {
        fn save(&self, task_id: &str, spec_json: &str) -> Result<(), String> {
            *self.saved_task_id.borrow_mut() = Some(task_id.to_string());
            *self.saved_json.borrow_mut() = Some(spec_json.to_string());
            Ok(())
        }

        fn load(&self, _task: &QueuedTask) -> Result<TaskSpec, String> {
            Err("not used in submit test".to_string())
        }
    }

    fn sample_run_state_draft() -> RunStateDraft {
        RunStateDraft {
            task_id: TaskId::new("task-22".to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(2).unwrap(),
            timeout_seconds: TimeoutSeconds::new(120).unwrap(),
            placement: Placement::new(
                vec!["local-coder".to_string()],
                vec!["gpu-2060".to_string()],
            ),
        }
    }
}
