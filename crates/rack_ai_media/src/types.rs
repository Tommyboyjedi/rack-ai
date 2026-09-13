use rack_ai_application::LeaseHandle;
use serde::{Deserialize, Serialize};
pub const VERSION: &str = "rack-ai/media/v1";
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ImageParameters {
    pub prompt: String,
    pub negative_prompt: String,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JobRequest {
    pub schema: String,
    pub work_id: String,
    pub submission_id: String,
    pub idempotency_key: String,
    pub profile: String,
    pub profile_version: u32,
    pub parameters: ImageParameters,
    pub timeout_seconds: u64,
    pub priority: Priority,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Medium,
    High,
    Paramount,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Waiting,
    Starting,
    Dispatching,
    Running,
    SubmissionUncertain,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}
impl JobState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Artifact {
    pub id: String,
    pub filename: String,
    pub sha256: String,
    pub bytes: u64,
    pub content_type: String,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Job {
    pub id: String,
    pub owner: String,
    pub request: JobRequest,
    pub state: JobState,
    pub created_at: u64,
    pub updated_at: u64,
    pub deadline: u64,
    pub cleanup_pending: bool,
    pub cancel_requested: bool,
    pub prompt_id: String,
    pub activation: Option<String>,
    pub workflow: serde_json::Value,
    pub workflow_sha256: String,
    pub model_identity: String,
    pub dispatched_at: Option<u64>,
    pub artifacts: Vec<Artifact>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    pub schema: String,
    pub idempotency_key: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    pub id: String,
    pub owner: String,
    pub request: SessionRequest,
    pub created_at: u64,
    pub release_requested: bool,
    pub stopped: bool,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Stopped,
    Reserving,
    Starting,
    Ready,
    Restarting,
    Draining,
    Stopping,
    Waiting,
    RecoveryRequired,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Closed,
    Interactive,
    Managed,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Service {
    pub session: Option<String>,
    pub state: ServiceState,
    pub mode: Mode,
    pub activation: String,
    pub invocation: Option<String>,
    #[serde(default)]
    pub generation: Option<crate::backend_generation::BackendGeneration>,
    #[serde(default)]
    pub restart: Option<crate::restart_state::RestartIntent>,
    pub lease: Option<LeaseHandle>,
    pub since: u64,
    pub heartbeat: u64,
    pub last_busy: u64,
    pub error: Option<String>,
}
impl Default for Service {
    fn default() -> Self {
        Self {
            session: None,
            state: ServiceState::Stopped,
            mode: Mode::Closed,
            activation: String::new(),
            invocation: None,
            generation: None,
            restart: None,
            lease: None,
            since: 0,
            heartbeat: 0,
            last_busy: 0,
            error: None,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BrowserSession {
    #[serde(default)]
    pub auth_generation: String,
    pub digest: String,
    pub owner: String,
    pub expires: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MediaState {
    pub schema: String,
    pub service: Service,
    pub jobs: Vec<Job>,
    pub sessions: Vec<Session>,
    pub browsers: Vec<BrowserSession>,
}
impl Default for MediaState {
    fn default() -> Self {
        Self {
            schema: VERSION.into(),
            service: Service::default(),
            jobs: vec![],
            sessions: vec![],
            browsers: vec![],
        }
    }
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
pub fn identity() -> String {
    uuid::Uuid::new_v4().to_string()
}
