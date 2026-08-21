use std::collections::HashSet;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignRepository {
    pub id: String,
    pub base_ref: String,
    pub base_sha: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignLimits {
    pub max_runtime_seconds: u64,
    pub max_steps: usize,
    pub max_total_attempts: usize,
    pub heartbeat_seconds: u64,
    pub network: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkerPolicy {
    pub primary: String,
    pub fallback: String,
    pub primary_attempts: usize,
    pub repair_attempts: usize,
    pub fallback_attempts: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepAcceptance {
    pub commands: Vec<Vec<String>>,
    #[serde(default)]
    pub required_artifacts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepLimits {
    pub timeout_seconds: u64,
    pub network: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CampaignStepKind {
    Implementation,
    Verification,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignStep {
    pub id: String,
    pub kind: CampaignStepKind,
    pub task: String,
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub required_changed_paths: Vec<String>,
    pub acceptance: StepAcceptance,
    pub limits: StepLimits,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Campaign {
    pub version: String,
    pub campaign_id: String,
    pub repository: CampaignRepository,
    pub branch: String,
    pub permitted_paths: Vec<String>,
    pub allow_local_commits: bool,
    pub limits: CampaignLimits,
    pub worker_policy: WorkerPolicy,
    pub steps: Vec<CampaignStep>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CampaignState {
    Paused,
    Running,
    Completed,
    Failed,
    Blocked,
    Cancelled,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepAttemptRecord {
    pub attempt: usize,
    pub worker_id: String,
    pub start_time: String,
    pub end_time: String,
    pub disposition: String,
    pub rationale: String,
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub repair_instruction: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepStatusRecord {
    pub step_id: String,
    pub kind: String,
    pub disposition: String,
    pub attempts: Vec<StepAttemptRecord>,
    pub accepted_commit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignStatus {
    pub schema_version: String,
    pub campaign_id: String,
    pub campaign_digest: String,
    pub repository_id: String,
    pub base_sha: String,
    pub branch: String,
    pub worktree_path: String,
    pub current_head_sha: String,
    pub state: CampaignState,
    pub current_step_id: Option<String>,
    pub current_attempt: usize,
    pub pause_requested: bool,
    pub cancel_requested: bool,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_seconds: u64,
    pub remaining_seconds: u64,
    pub last_heartbeat: String,
    pub steps: Vec<StepStatusRecord>,
    #[serde(default)]
    pub active_container_id: Option<String>,
    pub error_message: Option<String>,
    pub blocked_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignEvent {
    pub timestamp: String,
    pub campaign_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<usize>,
    pub event_type: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub details: serde_json::Map<String, Value>,
}

impl Campaign {
    pub fn validate_permitted_paths(&self) -> Result<(), String> {
        let permitted = self
            .permitted_paths
            .iter()
            .map(|value| value.as_str())
            .collect::<HashSet<_>>();
        for step in &self.steps {
            for path in &step.allowed_paths {
                if !is_permitted_path(path, &permitted) {
                    return Err(format!(
                        "step {} allowed_path '{}' violates campaign permitted_paths",
                        step.id, path
                    ));
                }
            }
            for path in &step.required_changed_paths {
                if !is_permitted_path(path, &permitted) {
                    return Err(format!(
                        "step {} required_changed_path '{}' violates campaign permitted_paths",
                        step.id, path
                    ));
                }
            }
        }
        Ok(())
    }
}

fn is_permitted_path(path: &str, permitted: &HashSet<&str>) -> bool {
    permitted.iter().any(|prefix| path.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::Campaign;
    use super::CampaignLimits;
    use super::CampaignRepository;
    use super::CampaignStep;
    use super::CampaignStepKind;
    use super::StepAcceptance;
    use super::StepLimits;
    use super::WorkerPolicy;

    #[test]
    fn rejects_step_paths_outside_campaign_allowlist() {
        let campaign = sample_campaign("src/domain/", "README.md");
        let error = campaign.validate_permitted_paths().unwrap_err();
        assert!(error.contains("allowed_path"));
    }

    #[test]
    fn accepts_required_changed_paths_within_allowlist() {
        let campaign = sample_campaign("src/domain/", "src/domain/entity.rs");
        assert!(campaign.validate_permitted_paths().is_ok());
    }

    fn sample_campaign(allowed_path: &str, required_changed_path: &str) -> Campaign {
        Campaign {
            version: "rack-ai/campaign/v1".to_string(),
            campaign_id: "campaign-1".to_string(),
            repository: CampaignRepository {
                id: "adaptos".to_string(),
                base_ref: "main".to_string(),
                base_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            },
            branch: "rack/campaign-campaign-1".to_string(),
            permitted_paths: vec!["src/domain/".to_string()],
            allow_local_commits: true,
            limits: CampaignLimits {
                max_runtime_seconds: 600,
                max_steps: 4,
                max_total_attempts: 4,
                heartbeat_seconds: 30,
                network: "disabled".to_string(),
            },
            worker_policy: WorkerPolicy {
                primary: "local-coder".to_string(),
                fallback: "local-primary".to_string(),
                primary_attempts: 1,
                repair_attempts: 1,
                fallback_attempts: 1,
            },
            steps: vec![CampaignStep {
                id: "step-1".to_string(),
                kind: CampaignStepKind::Implementation,
                task: "Do work.".to_string(),
                allowed_paths: vec![allowed_path.to_string()],
                required_changed_paths: vec![required_changed_path.to_string()],
                acceptance: StepAcceptance {
                    commands: vec![vec!["cargo".to_string(), "test".to_string()]],
                    required_artifacts: vec!["src/domain/mod.rs".to_string()],
                },
                limits: StepLimits {
                    timeout_seconds: 300,
                    network: "disabled".to_string(),
                },
            }],
        }
    }
}
