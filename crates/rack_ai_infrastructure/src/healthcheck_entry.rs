use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthcheckEntry {
    pub worker_id: String,
    pub enabled: bool,
    pub backend: String,
    pub resource_id: String,
    pub resource_status: Option<String>,
    pub model_id: String,
    pub model_status: Option<String>,
    pub endpoint_ok: Option<bool>,
}
