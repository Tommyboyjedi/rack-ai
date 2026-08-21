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
        let output = DirectCoderWorker::local_default().execute_with_runner(
            &CoderRunRequest::new(request.task().to_string(), request.max_turns())?
                .with_timeout_seconds(request.timeout_seconds()),
            &runner,
        )?;
        Ok(ImplementChangeResult::new(output))
    }
}
