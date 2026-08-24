use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::campaign_paths::parse_campaign_path;
use crate::campaign_paths::path_is_under_prefix;

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignRevisionDocument {
    pub instruction: String,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinatorReviewDisposition {
    Accepted,
    RejectedRetryable,
    RejectedTerminal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClassification {
    ToolProtocolViolation,
    NoChange,
    PathPolicyFailed,
    AcceptanceFailed,
    ArtifactMissing,
    WorkerTimeout,
    ReviewerTimeout,
    ReviewerFailure,
    RecoveryFailure,
    ExecutorUnavailable,
    ModelUnavailable,
    CampaignExpired,
    OperatorPaused,
    OperatorCancelled,
    ContinuityFailed,
    InsufficientAuthority,
    InadequateImplementation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptKind {
    Primary,
    Repair,
    Fallback,
    Verification,
}

impl FailureClassification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ToolProtocolViolation => "tool_protocol_violation",
            Self::NoChange => "no_change",
            Self::PathPolicyFailed => "path_policy_failed",
            Self::AcceptanceFailed => "acceptance_failed",
            Self::ArtifactMissing => "artifact_missing",
            Self::WorkerTimeout => "worker_timeout",
            Self::ReviewerTimeout => "reviewer_timeout",
            Self::ReviewerFailure => "reviewer_failure",
            Self::RecoveryFailure => "recovery_failure",
            Self::ExecutorUnavailable => "executor_unavailable",
            Self::ModelUnavailable => "model_unavailable",
            Self::CampaignExpired => "campaign_expired",
            Self::OperatorPaused => "operator_paused",
            Self::OperatorCancelled => "operator_cancelled",
            Self::ContinuityFailed => "continuity_failed",
            Self::InsufficientAuthority => "insufficient_authority",
            Self::InadequateImplementation => "inadequate_implementation",
        }
    }

    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::ToolProtocolViolation
                | Self::NoChange
                | Self::AcceptanceFailed
                | Self::ArtifactMissing
                | Self::WorkerTimeout
                | Self::ModelUnavailable
                | Self::InadequateImplementation
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CoordinatorReview {
    pub disposition: CoordinatorReviewDisposition,
    pub classification: Option<FailureClassification>,
    pub rationale: String,
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_instruction: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepAttemptRecord {
    pub attempt: usize,
    pub worker_id: String,
    pub kind: AttemptKind,
    pub start_time: String,
    pub end_time: String,
    pub disposition: CoordinatorReviewDisposition,
    pub classification: Option<FailureClassification>,
    pub rationale: String,
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub repair_instruction: Option<String>,
    #[serde(default)]
    pub next_repair_instruction: Option<String>,
    #[serde(default)]
    pub repair_of: Option<usize>,
    #[serde(default)]
    pub fallback_of: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepStatusRecord {
    pub step_id: String,
    pub kind: String,
    pub disposition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_disposition: Option<CoordinatorReviewDisposition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_rationale: Option<String>,
    pub attempts: Vec<StepAttemptRecord>,
    pub accepted_commit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RevisionRecord {
    pub instruction: String,
    pub added_step_ids: Vec<String>,
    pub recorded_at: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_worker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_action: Option<String>,
    pub pause_requested: bool,
    pub cancel_requested: bool,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_seconds: u64,
    pub remaining_seconds: u64,
    pub last_heartbeat: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_progress_time: Option<String>,
    pub steps: Vec<StepStatusRecord>,
    #[serde(default)]
    pub revisions: Vec<RevisionRecord>,
    #[serde(default)]
    pub active_lease_id: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub event_type: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub details: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignWorkerRuntime {
    pub worker_id: String,
    pub endpoint: String,
    pub api_model_id: String,
    pub entrypoint: String,
    pub provider_profile: String,
    pub tool_profile: Option<String>,
}

impl CampaignWorkerRuntime {
    pub fn implement_worker(&self) -> crate::ImplementWorkerRuntime {
        crate::ImplementWorkerRuntime::new(
            self.worker_id.clone(),
            self.entrypoint.clone(),
            self.provider_profile.clone(),
            self.api_model_id.clone(),
            self.endpoint.clone(),
        )
        .with_tool_profile(self.tool_profile.clone())
    }
}

pub trait CampaignWorkerCatalog {
    fn runtime(&self, worker_id: &str) -> Result<CampaignWorkerRuntime, String>;
}

pub trait CampaignHealth {
    fn assert_workers(&self, primary: &str, fallback: &str) -> Result<(), String>;
    fn assert_worker(&self, worker_id: &str) -> Result<(), String>;
    fn assert_executor(&self) -> Result<(), String>;
}

pub trait UnixClock {
    fn now_unix(&self) -> u64;
}

pub struct SystemUnixClock;

pub trait RecoverySleeper {
    fn sleep_seconds(&self, seconds: u64);
}

pub struct SystemRecoverySleeper;

impl RecoverySleeper for SystemRecoverySleeper {
    fn sleep_seconds(&self, seconds: u64) {
        std::thread::sleep(std::time::Duration::from_secs(seconds));
    }
}

impl UnixClock for SystemUnixClock {
    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

impl Campaign {
    pub fn expected_branch(campaign_id: &str) -> String {
        format!("rack/campaign-{campaign_id}")
    }

    pub fn validate_permitted_paths(&self) -> Result<(), String> {
        for path in &self.permitted_paths {
            parse_campaign_path(path)?;
        }
        for step in &self.steps {
            assert_step_paths_permitted(
                &step.id,
                &step.allowed_paths,
                &step.required_changed_paths,
                &self.permitted_paths,
            )?;
        }
        Ok(())
    }
}

pub fn assert_step_paths_permitted(
    step_id: &str,
    allowed_paths: &[String],
    required_changed_paths: &[String],
    permitted: &[String],
) -> Result<(), String> {
    for path in allowed_paths {
        parse_campaign_path(path)?;
        if !path_covered_by_permitted(path, permitted)? {
            return Err(format!(
                "step {step_id} allowed_path '{path}' violates campaign permitted_paths"
            ));
        }
    }
    for path in required_changed_paths {
        parse_campaign_path(path)?;
        if !path_covered_by_permitted(path, permitted)? {
            return Err(format!(
                "step {step_id} required_changed_path '{path}' violates campaign permitted_paths"
            ));
        }
    }
    Ok(())
}

fn path_covered_by_permitted(path: &str, permitted: &[String]) -> Result<bool, String> {
    for prefix in permitted {
        if path_is_under_prefix(path, prefix)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn campaign_digest(campaign: &Campaign) -> Result<String, String> {
    let json = serde_json::to_string(campaign).map_err(|error| error.to_string())?;
    Ok(format!("{:016x}", fnv1a64(json.as_bytes())))
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn source_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| !crate::ChangeLayout::is_ephemeral_path(path))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Campaign;
    use super::CampaignLimits;
    use super::CampaignRepository;
    use super::CampaignStep;
    use super::CampaignStepKind;
    use super::FailureClassification;
    use super::StepAcceptance;
    use super::StepLimits;
    use super::WorkerPolicy;

    #[test]
    fn rejects_step_paths_outside_campaign_allowlist() {
        let campaign = sample_campaign("README.md", "src/domain/entity.rs");
        let error = campaign.validate_permitted_paths().unwrap_err();
        assert!(error.contains("allowed_path"));
    }

    #[test]
    fn rejects_ambiguous_prefix_that_raw_starts_with_would_allow() {
        let mut campaign = sample_campaign("srcfoo/", "srcfoo/lib.rs");
        campaign.permitted_paths = vec!["src/".to_string()];
        let error = campaign.validate_permitted_paths().unwrap_err();
        assert!(error.contains("allowed_path"));
    }

    #[test]
    fn accepts_required_changed_paths_within_allowlist() {
        let campaign = sample_campaign("src/domain/", "src/domain/entity.rs");
        assert!(campaign.validate_permitted_paths().is_ok());
    }

    #[test]
    fn path_policy_and_expiry_are_not_retryable() {
        assert!(!FailureClassification::PathPolicyFailed.retryable());
        assert!(!FailureClassification::ContinuityFailed.retryable());
        assert!(!FailureClassification::ExecutorUnavailable.retryable());
        assert!(!FailureClassification::CampaignExpired.retryable());
        assert!(FailureClassification::NoChange.retryable());
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
