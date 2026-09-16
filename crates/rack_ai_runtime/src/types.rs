use rack_ai_application::{GenericCapability, GenericPriority};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const VERSION: &str = "rack-ai/runtime/v1";
pub type Priority = GenericPriority;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Acquire {
    pub schema: String,
    pub source_system: String,
    pub work_id: String,
    pub acquisition_id: String,
    pub tag: String,
    pub priority: Option<Priority>,
    pub capabilities: Vec<GenericCapability>,
    pub context_tokens: u32,
    pub ttl_seconds: u64,
    pub qualification: bool,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DemandState {
    Denied,
    Preparing,
    Ready,
    Draining,
    Held,
    Releasing,
    Released,
    Cancelled,
    Expired,
    RecoveryRequired,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Demand {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve_request: Option<crate::reservation::Reserve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve_result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_closed: Option<DemandState>,
    pub id: String,
    pub owner: String,
    pub request: Acquire,
    pub priority: Priority,
    pub profile: crate::config::Profile,
    pub profile_hash: String,
    pub state: DemandState,
    pub reason: Option<String>,
    pub generation: String,
    pub access_key: String,
    pub created: u64,
    #[serde(default)]
    pub last_activity_at: Option<u64>,
    pub order: u64,
    pub deadline: u64,
    pub transition_deadline: u64,
    pub victims: Vec<String>,
    pub process: Option<Process>,
    pub effect_started: bool,
    pub preflight_done: bool,
    pub released: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Process {
    pub pid: u32,
    pub boot: String,
    pub start: String,
    pub activation: String,
    pub unit: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    pub invocation: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Inference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<crate::work_payload::Work>,
    pub schema: String,
    pub submission_id: String,
    pub reservation_id: String,
    pub generation: String,
    pub profile_hash: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<crate::protocol::Payload>,
    pub max_tokens: u32,
    pub timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_scope: Option<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvocationState {
    Accepted,
    Started,
    Completed,
    Cancelled,
    Expired,
    Uncertain,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CancellationIntent {
    pub requested_at: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Invocation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_access_hash: Option<String>,
    pub id: String,
    pub owner: String,
    pub request: Inference,
    pub state: InvocationState,
    #[serde(alias = "deadline")]
    pub waiting_deadline: u64,
    #[serde(default)]
    pub execution_deadline: Option<u64>,
    #[serde(default = "legacy_response_bound")]
    pub response_bytes: u64,
    #[serde(default)]
    pub cancellation: Option<CancellationIntent>,
    #[serde(default)]
    pub late_result: Option<serde_json::Value>,
    pub started: Option<u64>,
    pub activation: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}
#[derive(Default, Deserialize, Serialize)]
pub struct State {
    #[serde(default)]
    pub placement_hash: Option<String>,
    #[serde(default)]
    pub gateway_port: Option<u16>,
    pub demands: BTreeMap<String, Demand>,
    pub invocations: BTreeMap<String, Invocation>,
    #[serde(default)]
    pub workspace_scopes: BTreeMap<String, crate::workspace_scope::WorkspaceScope>,
}
pub type Document = rack_ai_infrastructure::managed_authority::AuthorityDocument<State>;
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn identity() -> Result<String, String> {
    rack_ai_infrastructure::resource_reservations::new_identity()
}
pub fn digest(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

fn legacy_response_bound() -> u64 {
    4 * 1024 * 1024
}
impl Invocation {
    pub fn cancel(&mut self) {
        if matches!(
            self.state,
            InvocationState::Accepted | InvocationState::Started | InvocationState::Uncertain
        ) {
            self.cancellation.get_or_insert(CancellationIntent {
                requested_at: now(),
            });
            if self.state == InvocationState::Accepted {
                self.state = InvocationState::Cancelled;
            }
        }
    }
}
