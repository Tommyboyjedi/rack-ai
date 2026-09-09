use rack_ai_application::WorkerExecutionProvenance;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WorkerRecord {
    pub id: String,
    pub kind: String,
    pub role: String,
    pub entrypoint: String,
    pub backend: String,
    pub resource_id: String,
    pub model_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub provider_profile: Option<String>,
    #[serde(default)]
    pub tool_profile: Option<String>,
}

impl WorkerRecord {
    pub fn execution_provenance(&self) -> Result<WorkerExecutionProvenance, String> {
        let provider_profile = self
            .provider_profile
            .clone()
            .ok_or_else(|| format!("worker missing provider_profile: {}", self.id))?;
        Ok(WorkerExecutionProvenance {
            worker_id: self.id.clone(),
            worker_role: self.role.clone(),
            worker_kind: self.kind.clone(),
            model_id: self.model_id.clone(),
            provider_profile,
            resource_id: self.resource_id.clone(),
            backend: self.backend.clone(),
            tool_profile: self
                .tool_profile
                .clone()
                .filter(|value| !value.trim().is_empty()),
        })
    }
}
