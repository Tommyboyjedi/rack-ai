use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkUnitComplexity {
    Small,
    Medium,
    Large,
}

impl WorkUnitComplexity {
    pub fn prefers_stronger_worker(&self) -> bool {
        matches!(self, Self::Medium | Self::Large)
    }
}
