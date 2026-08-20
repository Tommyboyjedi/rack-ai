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
}

impl<'a> SubmitTask<'a> {
    pub fn new(repositories: SubmitTaskDependencies<'a>) -> Self {
        Self {
            run_state_repository: repositories.run_state_repository,
            task_spec_repository: repositories.task_spec_repository,
        }
    }

    pub fn execute(&self, request: SubmitTaskRequest) -> Result<RunState, String> {
        let run_state = RunState::queued(request.run_state);
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
    use crate::RunStateRepository;
    use crate::TaskSpecRepository;

    #[test]
    fn saves_spec_and_run_state() {
        let run_states = RefCell::new(Vec::<RunState>::new());
        let specs = RefCell::new(Vec::<(String, String)>::new());
        let run_state_repository = FakeRunStateRepository {
            run_states: &run_states,
        };
        let task_spec_repository = FakeTaskSpecRepository { specs: &specs };
        let submit_task = SubmitTask::new(SubmitTaskDependencies {
            run_state_repository: &run_state_repository,
            task_spec_repository: &task_spec_repository,
        });

        let result = submit_task.execute(sample_request()).unwrap();

        assert_eq!(result.task_id().value(), "task-42");
        assert_eq!(run_states.borrow().len(), 1);
        assert_eq!(specs.borrow().len(), 1);
    }

    struct FakeRunStateRepository<'a> {
        run_states: &'a RefCell<Vec<RunState>>,
    }

    impl RunStateRepository for FakeRunStateRepository<'_> {
        fn save(&self, run_state: &RunState) -> Result<(), String> {
            self.run_states.borrow_mut().push(run_state.clone());
            Ok(())
        }

        fn find(&self, _task_id: &TaskId) -> Result<Option<RunState>, String> {
            Ok(None)
        }
    }

    struct FakeTaskSpecRepository<'a> {
        specs: &'a RefCell<Vec<(String, String)>>,
    }

    impl TaskSpecRepository for FakeTaskSpecRepository<'_> {
        fn save(&self, task_id: &str, spec_json: &str) -> Result<(), String> {
            self.specs
                .borrow_mut()
                .push((task_id.to_string(), spec_json.to_string()));
            Ok(())
        }
    }

    fn sample_request() -> SubmitTaskRequest {
        SubmitTaskRequest {
            spec_json: "{\"task_id\":\"task-42\"}".to_string(),
            run_state: RunStateDraft {
                task_id: TaskId::new("task-42".to_string()).unwrap(),
                attempt_limit: AttemptLimit::new(1).unwrap(),
                timeout_seconds: TimeoutSeconds::new(120).unwrap(),
                placement: Placement::new(vec!["worker-a".to_string()], vec!["gpu-a".to_string()]),
            },
        }
    }
}
