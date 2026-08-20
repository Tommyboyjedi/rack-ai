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
        self.task_spec_repository
            .save(run_state.task_id().value(), request.spec_json.as_str())?;
        self.run_state_repository.save(&run_state)?;
        Ok(run_state)
    }
}
