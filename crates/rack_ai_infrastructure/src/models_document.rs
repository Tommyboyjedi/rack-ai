use serde::Deserialize;

use crate::ModelRecord;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ModelsDocument {
    pub models: Vec<ModelRecord>,
}
