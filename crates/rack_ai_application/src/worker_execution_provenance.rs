use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkerExecutionProvenance {
    pub worker_id: String,
    pub worker_role: String,
    pub worker_kind: String,
    pub model_id: String,
    pub provider_profile: String,
    pub resource_id: String,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_profile: Option<String>,
}
