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
}
