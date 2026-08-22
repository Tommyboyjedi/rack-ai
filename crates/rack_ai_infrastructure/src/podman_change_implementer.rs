use rack_ai_application::ChangeImplementer;
use rack_ai_application::CoderRunRequest;
use rack_ai_application::CoderWorkspaceContext;
use rack_ai_application::ImplementChangeRequest;
use rack_ai_application::ImplementChangeResult;
use rack_ai_application::looks_like_markdown_tool_call;

use crate::DirectCoderWorker;
use crate::PodmanWorkspaceExecutor;
use crate::RecordingCoderToolRunner;
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
        let workspace_runner = WorkspaceCoderToolRunner::new(
            &self.executor,
            CoderWorkspaceContext::new(
                request.worktree_path().to_path_buf(),
                request.allowed_paths()?.clone(),
            )
            .with_timeout_seconds(request.timeout_seconds()),
        );
        let runner = RecordingCoderToolRunner::new(&workspace_runner);
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
        );
        let tool_calls = runner.calls();
        match output {
            Ok(text) => {
                let mut result = ImplementChangeResult::new(text.clone())
                    .with_tool_calls(tool_calls)
                    .with_executor_kind("podman".to_string());
                if looks_like_markdown_tool_call(&text) {
                    result = result.with_protocol_error(
                        "worker emitted markdown or JSON text instead of a valid tool call"
                            .to_string(),
                    );
                }
                Ok(result)
            }
            Err(error) => {
                let lower = error.to_lowercase();
                let mut result = ImplementChangeResult::new(String::new())
                    .with_tool_calls(tool_calls)
                    .with_executor_kind("podman".to_string());
                if lower.contains("tool")
                    || lower.contains("finish_reason")
                    || lower.contains("markdown")
                {
                    result = result.with_protocol_error(error);
                } else {
                    result = result.with_worker_error(error);
                }
                Ok(result)
            }
        }
    }
}
