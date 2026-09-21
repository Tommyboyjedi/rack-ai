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
    /// A new acquisition could not take ownership now. This is never queued
    /// inference work.
    #[serde(alias = "denied")]
    Unavailable,
    Preparing,
    Ready,
    /// A higher-priority owner is waiting for work already running under this
    /// claim to drain. New work is rejected and queued work is cancelled.
    #[serde(alias = "draining")]
    Preempting,
    /// Ownership was displaced. It is terminal; only a new acquisition can
    /// obtain the service again.
    #[serde(alias = "held")]
    Preempted,
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
    /// Durable evidence that an historically uncertain start no longer has a
    /// current physical effect. It never changes the historical invocation or
    /// start outcome into a success, failure, cancellation, or replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_reconciliation: Option<RecoveryReconciliation>,
    /// Latest cleanup blocker, separate from the original historical failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_error: Option<String>,
    /// Workspace-specific diagnosis that proves an uncertain workspace has no
    /// live scoped model work left, without rewriting its historical outcome.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workspace_recovery_analyses: BTreeMap<String, WorkspaceRecoveryAnalysis>,
    pub id: String,
    pub owner: String,
    pub request: Acquire,
    pub priority: Priority,
    pub profile: crate::config::Profile,
    pub profile_hash: String,
    pub state: DemandState,
    pub reason: Option<String>,
    /// A bounded hint returned with an unavailable acquisition. It is not a
    /// lease or a promise that a future acquisition will succeed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    /// The demand that superseded this ownership claim, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preempted_by: Option<String>,
    /// Readiness was proven for this member; multi-service reservations commit
    /// every member Ready together only after all checks have succeeded.
    #[serde(default)]
    pub ready_checked: bool,
    /// Bounded lifetime call admission counter. It is a documented reservation
    /// capacity, not a retained-evidence accident.
    #[serde(default)]
    pub accepted_calls: u64,
    pub generation: String,
    pub access_key: String,
    /// RackAI-owned backend/process identity. Legacy records fall back to generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_activation: Option<String>,
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
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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
#[serde(rename_all = "snake_case")]
pub enum WarmResidencyState {
    Resident,
    Evicting,
    RecoveryRequired,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WarmResidency {
    pub id: String,
    pub profile_hash: String,
    pub profile: crate::config::Profile,
    pub backend: crate::config::Backend,
    pub driver: crate::config::Driver,
    pub model: String,
    pub resources: Vec<String>,
    pub endpoint: String,
    pub process: Process,
    pub cached_at: u64,
    pub last_ready_at: u64,
    pub state: WarmResidencyState,
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
    #[serde(alias = "accepted")]
    Queued,
    #[serde(alias = "started")]
    Running,
    Completed,
    Cancelled,
    Failed,
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
    /// SHA-256 of the original full request before terminal request compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_digest: Option<String>,
    /// Serialized byte length of the original full request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_bytes: Option<u64>,
    /// SHA-256 of the original full workspace/work request, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_digest: Option<String>,
    /// Serialized byte length of the original full workspace/work request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_bytes: Option<u64>,
    pub state: InvocationState,
    /// Monotonic local queue position. Dispatch is FIFO within one owned
    /// logical service and never uses global reservation priority.
    #[serde(default)]
    pub queue_order: u64,
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
    /// Content digests preserve audit/reconciliation after bounded terminal
    /// payload compaction without pretending the original response is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub late_result_digest: Option<String>,
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
    /// Monotonic sequencing survives a receiver restart. Legacy records that
    /// lack a sequence retain their deterministic id tie-breaker.
    #[serde(default)]
    pub next_invocation_order: u64,
    /// Short-lived, bounded negative acquisition decisions. They are control
    /// plane cache entries rather than reservations or retained work evidence.
    #[serde(default)]
    pub acquisition_decisions: BTreeMap<String, AcquisitionDecision>,
    #[serde(default)]
    pub workspace_scopes: BTreeMap<String, crate::workspace_scope::WorkspaceScope>,
    #[serde(default)]
    pub warm_residencies: BTreeMap<String, WarmResidency>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AcquisitionDecision {
    pub owner: String,
    pub environment: String,
    pub reason: String,
    pub retry_after: u64,
    pub expires_at: u64,
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
pub fn json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|e| e.to_string())
}
pub fn json_digest<T: Serialize>(value: &T) -> Result<String, String> {
    Ok(digest(&json_bytes(value)?))
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

impl Demand {
    pub fn backend_activation(&self) -> &str {
        self.backend_activation
            .as_deref()
            .unwrap_or(&self.generation)
    }
}

/// The result of the original start attempt. This is historical evidence and is
/// deliberately independent of whether the physical effect is still present.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalOutcome {
    StartOutcomeUnknown,
    RecoveryOutcomeUnknown,
}

/// The current physical-effect conclusion made by an evidence-backed recovery
/// probe. Additional values must never be inferred from a missing PID alone.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CurrentEffect {
    ProvenAbsent,
}

/// Every check is recorded only after all probes succeeded. The durable record
/// distinguishes an unknown historical start result from an absent current
/// effect, so receiver restart/resume cannot recreate a released claim.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RecoveryAbsenceChecks {
    pub reservation_inactive: bool,
    pub active_invocations_absent: bool,
    pub recorded_process_absent: bool,
    pub systemd_activation_absent: bool,
    pub gpu_allocation_absent: bool,
    pub media_session_absent: bool,
    pub lifecycle_transition_absent: bool,
    pub ownership_fence_intact: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RecoveryReconciliation {
    pub historical_outcome: HistoricalOutcome,
    pub current_effect: CurrentEffect,
    pub reconciled_at: u64,
    pub checks: RecoveryAbsenceChecks,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_process: Option<Process>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkspaceRecoveryAnalysis {
    pub invocation_id: String,
    pub diagnosed_at: u64,
    pub parent_error: Option<String>,
    pub work_id: Option<String>,
    pub repository_id: Option<String>,
    pub repository_root: Option<String>,
    pub packet_path: String,
    pub packet_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_acceptance_verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_last_error: Option<String>,
    pub scoped_children: Vec<WorkspaceRecoveryChild>,
    pub checks: WorkspaceRecoveryChecks,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkspaceRecoveryChild {
    pub invocation_id: String,
    pub state: InvocationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_scope: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reservation_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_generation: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_profile_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_activation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand_backend_activation: Option<String>,
    #[serde(default)]
    pub physical_effect: WorkspaceRecoveryChildPhysicalEffect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_deadline: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation_requested_at: Option<u64>,
    pub result_present: bool,
    pub late_result_present: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRecoveryChildPhysicalEffect {
    Terminal,
    ConfinedToDemandBackend,
    UnresolvedActive,
    IndependentUnproven,
}

impl Default for WorkspaceRecoveryChildPhysicalEffect {
    fn default() -> Self {
        Self::IndependentUnproven
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkspaceRecoveryChecks {
    /// The parent workspace has a retained review packet under the configured
    /// RackAI state root, so the workspace attempt itself reached a recorded
    /// terminal packet outcome.
    pub retained_terminal_packet: bool,
    /// No scoped child invocation for this workspace parent is queued, running,
    /// or uncertain.
    pub scoped_children_terminal: bool,
    /// Every scoped child is either logically terminal or historically
    /// uncertain but physically confined to the fenced demand backend that
    /// stop-only Recovery will tear down. The historical child outcome is not
    /// rewritten.
    #[serde(default)]
    pub scoped_children_physically_recoverable: bool,
    /// All workspace scopes owned by this parent are closed or past deadline.
    pub workspace_scope_closed_or_expired: bool,
    /// The retained packet path was canonicalized under workspace.state_root.
    pub packet_under_state_root: bool,
    /// The original recovery ownership fence still matched when analysis was
    /// persisted.
    pub ownership_fence_intact: bool,
}

fn legacy_response_bound() -> u64 {
    4 * 1024 * 1024
}
impl Invocation {
    pub fn cancel(&mut self) {
        if matches!(
            self.state,
            InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
        ) {
            self.cancellation.get_or_insert(CancellationIntent {
                requested_at: now(),
            });
            if self.state == InvocationState::Queued {
                self.state = InvocationState::Cancelled;
            }
        }
    }
    pub fn cancel_queued_as_superseded(&mut self) {
        if self.state == InvocationState::Queued {
            self.state = InvocationState::Cancelled;
            self.error = Some("reservation_superseded_by_higher_priority".into());
        }
    }
}
