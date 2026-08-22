use serde::Deserialize;
use serde::Serialize;

use crate::CampaignState;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignSupervisionAction {
    pub campaign_id: String,
    pub previous_state: CampaignState,
    pub action: String,
    pub outcome_state: Option<CampaignState>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignCleanupAction {
    pub campaign_id: String,
    pub action: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignSupervisionReport {
    pub schema_version: String,
    pub scanned_campaigns: usize,
    pub resumed_campaigns: usize,
    pub actions: Vec<CampaignSupervisionAction>,
    pub cleanup: Vec<CampaignCleanupAction>,
}

impl CampaignSupervisionReport {
    pub fn new() -> Self {
        Self {
            schema_version: "rack-ai/campaign-supervision-report/v1".to_string(),
            scanned_campaigns: 0,
            resumed_campaigns: 0,
            actions: Vec::new(),
            cleanup: Vec::new(),
        }
    }
}
