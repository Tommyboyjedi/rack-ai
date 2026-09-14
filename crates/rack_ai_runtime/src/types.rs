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
pub struct Invocation {
    pub id: String,
    pub owner: String,
    pub request: Inference,
    pub state: InvocationState,
    pub deadline: u64,
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
