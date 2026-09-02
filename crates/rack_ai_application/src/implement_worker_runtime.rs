use crate::WorkerExecutionProvenance;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementWorkerRuntime {
    worker_id: String,
    entrypoint: String,
    provider_profile: String,
    api_model_id: String,
    endpoint: String,
    tool_profile: Option<String>,
    context_window: Option<u32>,
    worker_provenance: Option<WorkerExecutionProvenance>,
}

impl ImplementWorkerRuntime {
    pub fn new(
        worker_id: String,
        entrypoint: String,
        provider_profile: String,
        api_model_id: String,
        endpoint: String,
    ) -> Self {
        Self {
            worker_id,
            entrypoint,
            provider_profile,
            api_model_id,
            endpoint,
            tool_profile: None,
            context_window: None,
            worker_provenance: None,
        }
    }

    pub fn with_worker_provenance(mut self, provenance: WorkerExecutionProvenance) -> Self {
        self.worker_provenance = Some(provenance);
        self
    }

    pub fn with_tool_profile(mut self, tool_profile: Option<String>) -> Self {
        self.tool_profile = tool_profile.filter(|value| !value.trim().is_empty());
        self
    }

    pub fn with_context_window(mut self, context_window: Option<u32>) -> Self {
        self.context_window = context_window;
        self
    }

    pub fn worker_id(&self) -> &str {
        self.worker_id.as_str()
    }

    pub fn entrypoint(&self) -> &str {
        self.entrypoint.as_str()
    }

    pub fn provider_profile(&self) -> &str {
        self.provider_profile.as_str()
    }

    pub fn api_model_id(&self) -> &str {
        self.api_model_id.as_str()
    }

    pub fn endpoint(&self) -> &str {
        self.endpoint.as_str()
    }

    pub fn tool_profile(&self) -> Option<&str> {
        self.tool_profile.as_deref()
    }

    pub fn context_window(&self) -> Option<u32> {
        self.context_window
    }

    pub fn worker_provenance(&self) -> Option<&WorkerExecutionProvenance> {
        self.worker_provenance.as_ref()
    }
}
