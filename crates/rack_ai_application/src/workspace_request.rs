//! Typed input to the reusable bounded change executor; no wire-version parser.
use crate::{ChangeRequestDocument, GenericRoutingHeader};
use rack_ai_domain::WorkUnitComplexity;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRequirements {
    #[serde(default = "small")]
    pub complexity: WorkUnitComplexity,
    #[serde(default)]
    pub requires_large_context: bool,
}
fn small() -> WorkUnitComplexity {
    WorkUnitComplexity::Small
}
impl Default for WorkspaceRequirements {
    fn default() -> Self {
        Self {
            complexity: small(),
            requires_large_context: false,
        }
    }
}
#[derive(Clone, Debug)]
pub struct WorkspaceRequest {
    pub change: ChangeRequestDocument,
    pub requirements: WorkspaceRequirements,
    pub routing: GenericRoutingHeader,
}
impl WorkspaceRequest {
    pub fn complexity(&self) -> WorkUnitComplexity {
        self.requirements.complexity
    }
    pub fn requires_large_context(&self) -> bool {
        self.requirements.requires_large_context
    }
    pub fn routing(&self) -> &GenericRoutingHeader {
        &self.routing
    }
    pub fn change_id(&self) -> String {
        self.change.change_id.clone()
    }
}
