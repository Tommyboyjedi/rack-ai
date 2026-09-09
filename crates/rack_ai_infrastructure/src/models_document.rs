use serde::Deserialize;

use rack_ai_application::GenericSourceAdmissionPolicy;

use crate::ModelRecord;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ModelsDocument {
    pub models: Vec<ModelRecord>,
    #[serde(default)]
    pub source_admission_policies: Vec<GenericSourceAdmissionPolicy>,
}
