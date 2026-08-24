use rack_ai_application::ChangeImplementer;
use rack_ai_application::ImplementChangeRequest;
use rack_ai_application::ImplementChangeResult;
use rack_ai_application::ImplementWorkerRuntime;

use crate::JCodeProcessRunner;
use crate::JCodeWorkerConfigResolver;
use crate::RegistryPaths;

pub struct JCodeChangeImplementer {
    resolver: JCodeWorkerConfigResolver,
    default_worker: Option<ImplementWorkerRuntime>,
}

impl JCodeChangeImplementer {
    pub fn new(paths: RegistryPaths, default_worker: Option<ImplementWorkerRuntime>) -> Self {
        Self {
            resolver: JCodeWorkerConfigResolver::new(paths),
            default_worker,
        }
    }

    fn resolve_runtime(&self, request: &ImplementChangeRequest) -> Result<ImplementWorkerRuntime, String> {
        if let Some(runtime) = request.worker() {
            let resolved = self.resolver.resolve(runtime.worker_id())?;
            if resolved.api_model_id() != runtime.api_model_id() {
                return Err(format!(
                    "worker model mismatch for {}: request={}, registry={}",
                    runtime.worker_id(),
                    runtime.api_model_id(),
                    resolved.api_model_id()
                ));
            }
            if resolved.endpoint() != runtime.endpoint() {
                return Err(format!(
                    "worker endpoint mismatch for {}: request={}, registry={}",
                    runtime.worker_id(),
                    runtime.endpoint(),
                    resolved.endpoint()
                ));
            }
            return Ok(resolved);
        }
        self.default_worker
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.resolver.resolve_default_implementer())
    }
}

impl ChangeImplementer for JCodeChangeImplementer {
    fn implement(&self, request: &ImplementChangeRequest) -> Result<ImplementChangeResult, String> {
        let runtime = self.resolve_runtime(request)?;
        let output = JCodeProcessRunner::run(
            &runtime,
            request.task(),
            request.worktree_path(),
            request.timeout_seconds(),
        );
        match output {
            Ok(result) => Ok(ImplementChangeResult::new(result.stdout().trim().to_string())
                .with_stderr(result.stderr().to_string())
                .with_executor_kind("jcode-direct".to_string())),
            Err(error) => Ok(ImplementChangeResult::new(String::new())
                .with_executor_kind("jcode-direct".to_string())
                .with_worker_error(error)),
        }
    }
}
