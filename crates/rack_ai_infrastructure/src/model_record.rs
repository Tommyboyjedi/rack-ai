use serde::Deserialize;

use rack_ai_application::GenericModelEligibilityProfile;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ModelRecord {
    pub id: String,
    pub label: String,
    pub role: String,
    pub backend: String,
    pub worker_id: String,
    pub endpoint: String,
    pub port: u32,
    pub status: String,
    #[serde(default)]
    pub api_model_id: Option<String>,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_num_seqs: Option<u32>,
    #[serde(default)]
    pub eligibility_profile: Option<GenericModelEligibilityProfile>,
}
