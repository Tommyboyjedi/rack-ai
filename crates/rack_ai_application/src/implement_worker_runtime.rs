#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementWorkerRuntime {
    worker_id: String,
    entrypoint: String,
    provider_profile: String,
    api_model_id: String,
    endpoint: String,
    tool_profile: Option<String>,
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
        }
    }

    pub fn with_tool_profile(mut self, tool_profile: Option<String>) -> Self {
        self.tool_profile = tool_profile.filter(|value| !value.trim().is_empty());
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
}
