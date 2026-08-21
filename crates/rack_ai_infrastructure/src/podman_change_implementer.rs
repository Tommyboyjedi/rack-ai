use rack_ai_application::ChangeImplementer;
use rack_ai_application::CoderRunRequest;
use rack_ai_application::CoderWorkspaceContext;
use rack_ai_application::ImplementChangeRequest;
use rack_ai_application::ImplementChangeResult;

use crate::DirectCoderWorker;
use crate::PodmanWorkspaceExecutor;
use crate::WorkspaceCoderToolRunner;

pub struct PodmanChangeImplementer {
    executor: PodmanWorkspaceExecutor,
}

impl PodmanChangeImplementer {
    pub fn new(executor: PodmanWorkspaceExecutor) -> Self {
        Self { executor }
    }
}

impl ChangeImplementer for PodmanChangeImplementer {
    fn implement(&self, request: &ImplementChangeRequest) -> Result<ImplementChangeResult, String> {
        let runner = WorkspaceCoderToolRunner::new(
            &self.executor,
            CoderWorkspaceContext::new(
                request.worktree_path().to_path_buf(),
                request.allowed_paths()?.clone(),
            )
            .with_timeout_seconds(request.timeout_seconds()),
        );
        let worker = if let (Some(endpoint), Some(model_id)) =
            (request.worker_endpoint(), request.worker_model_id())
        {
            DirectCoderWorker::new(
                endpoint.to_string(),
                model_id.to_string(),
                DirectCoderWorker::default_system_prompt(),
            )
        } else {
            DirectCoderWorker::local_default()
        };
        let output = worker.execute_with_runner(
            &CoderRunRequest::new(request.task().to_string(), request.max_turns())?
                .with_timeout_seconds(request.timeout_seconds()),
            &runner,
        )?;
        Ok(ImplementChangeResult::new(output))
    }
}
