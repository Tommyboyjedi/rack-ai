use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rack_ai_domain::AcceptanceCommand;
use rack_ai_domain::AllowedPath;
use rack_ai_domain::AllowedPaths;
use rack_ai_domain::GitRef;
use rack_ai_domain::GitSha;
use rack_ai_domain::RepositoryId;
use serde::Serialize;

use crate::AttemptKind;
use crate::Campaign;
use crate::CampaignCommitRequest;
use crate::CampaignContainerTracker;
use crate::CampaignEvent;
use crate::CampaignHealth;
use crate::CampaignLeaseStore;
use crate::CampaignLock;
use crate::CampaignRevisionDocument;
use crate::CampaignState;
use crate::CampaignStatus;
use crate::CampaignStep;
use crate::CampaignStepKind;
use crate::CampaignWorkerCatalog;
use crate::ChangeImplementer;
use crate::ChangeLayout;
use crate::CommandEvidence;
use crate::CommandPolicy;
use crate::CoordinatorReview;
use crate::CoordinatorReviewDisposition;
use crate::CreateChangeWorktreeRequest;
use crate::FailureClassification;
use crate::GitEvidence;
use crate::GitWorktree;
use crate::ImplementChangeRequest;
use crate::ImplementChangeResult;
use crate::ImplementationReviewer;
use crate::ModelReviewRequest;
use crate::ReadFileRequest;
use crate::RecoveryAttemptSummary;
use crate::RecoveryContext;
use crate::RecoveryDecision;
use crate::RecoveryDecisionKind;
use crate::RecoveryFailureKind;
use crate::RecoveryReasoner;
use crate::RecoveryReasoningRequest;
use crate::RecoveryReasoningResult;
use crate::RecoveryToolAttempt;
use crate::RecoveryWorkerAction;
use crate::RecoveryCommandFailure;
use crate::RecoverySleeper;
use crate::RepositoryRegistry;
use crate::ResolveGitShaRequest;
use crate::ReviewInput;
use crate::RevisionRecord;
use crate::RunCommandRequest;
use crate::StepAttemptRecord;
use crate::StepStatusRecord;
use crate::UnixClock;
use crate::WorkspaceExecutor;
use crate::WorkspacePath;

const REVIEWER_MIN_TIMEOUT_SECONDS: u32 = 180;
const REVIEWER_MAX_ATTEMPTS: usize = 3;
const REVIEWER_RETRY_DELAYS_SECONDS: [u64; 2] = [2, 5];
use crate::assert_step_paths_permitted;
use crate::atomic_write;
use crate::campaign_digest;
use crate::durable_file::append_line;
use crate::path_is_authorized;
use crate::repair_instruction;
use crate::review_attempt;
use crate::source_paths;

pub struct CampaignRunnerDependencies<'a> {
    pub registry: &'a dyn RepositoryRegistry,
    pub command_policy: &'a dyn CommandPolicy,
    pub git: &'a dyn GitWorktree,
    pub implementer: &'a dyn ChangeImplementer,
    pub executor: &'a dyn WorkspaceExecutor,
    pub workers: &'a dyn CampaignWorkerCatalog,
    pub health: &'a dyn CampaignHealth,
    pub clock: &'a dyn UnixClock,
    pub sleeper: &'a dyn RecoverySleeper,
    pub worker_recovery_max_wait_seconds: u64,
    pub worker_recovery_retry_delays_seconds: Vec<u64>,
    pub worker_recovery_max_attempts: usize,
    pub state_root: PathBuf,
    pub container_tracker: Option<Arc<CampaignContainerTracker>>,
}

pub struct CampaignRunner<'a> {
    registry: &'a dyn RepositoryRegistry,
    command_policy: &'a dyn CommandPolicy,
    git: &'a dyn GitWorktree,
    implementer: &'a dyn ChangeImplementer,
    executor: &'a dyn WorkspaceExecutor,
    workers: &'a dyn CampaignWorkerCatalog,
    health: &'a dyn CampaignHealth,
    clock: &'a dyn UnixClock,
    sleeper: &'a dyn RecoverySleeper,
    worker_recovery_max_wait_seconds: u64,
    worker_recovery_retry_delays_seconds: Vec<u64>,
    worker_recovery_max_attempts: usize,
    state_root: PathBuf,
    leases: CampaignLeaseStore,
    container_tracker: Option<Arc<CampaignContainerTracker>>,
    reviewer: Option<&'a dyn ImplementationReviewer>,
    recovery_reasoner: Option<&'a dyn RecoveryReasoner>,
}

struct StateHeartbeatGuard {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for StateHeartbeatGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Serialize)]
struct GitEvidenceDocument {
    head_sha: String,
    status: String,
    diff: String,
    diff_stat: String,
    changed_paths: Vec<String>,
}

#[derive(Serialize)]
struct ReviewPacketDocument {
    step_id: String,
    attempt: usize,
    worker_id: String,
    attempt_kind: AttemptKind,
    repair_instruction: Option<String>,
    next_repair_instruction: Option<String>,
    review: CoordinatorReview,
    changed_paths: Vec<String>,
    commit_sha: Option<String>,
}

#[derive(Serialize)]
struct RecoveryResetEvidenceDocument {
    step_id: String,
    attempt: usize,
    worker_id: Option<String>,
    action: String,
    reason: String,
    dirty_paths: Vec<String>,
    diff_stat: String,
    diff_excerpt: String,
    current_head_sha: String,
    worktree_path: String,
}

struct ReviewerFailure {
    classification: FailureClassification,
    rationale: String,
}

#[derive(Clone)]
struct RecoveryTrace {
    request: RecoveryReasoningRequest,
    result: Result<RecoveryReasoningResult, String>,
    fingerprint: String,
    repeated_failure_count: usize,
}

struct RecoveryPlanningOutcome {
    review: CoordinatorReview,
    next_instruction: Option<String>,
    prefer_fallback_worker: bool,
    trace: Option<RecoveryTrace>,
    fingerprint: String,
    repeated_failure_count: usize,
}

fn classify_reviewer_error(error: &str) -> FailureClassification {
    let lower = error.to_lowercase();
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("peer disconnected")
        || lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("transport")
    {
        FailureClassification::ReviewerTimeout
    } else {
        FailureClassification::ReviewerFailure
    }
}

impl<'a> CampaignRunner<'a> {
    pub fn new(dependencies: CampaignRunnerDependencies<'a>) -> Self {
        let leases = CampaignLeaseStore::new(dependencies.state_root.clone());
        Self {
            registry: dependencies.registry,
            command_policy: dependencies.command_policy,
            git: dependencies.git,
            implementer: dependencies.implementer,
            executor: dependencies.executor,
            workers: dependencies.workers,
            health: dependencies.health,
            clock: dependencies.clock,
            sleeper: dependencies.sleeper,
            worker_recovery_max_wait_seconds: dependencies.worker_recovery_max_wait_seconds,
            worker_recovery_retry_delays_seconds: dependencies.worker_recovery_retry_delays_seconds,
            worker_recovery_max_attempts: dependencies.worker_recovery_max_attempts,
            state_root: dependencies.state_root,
            leases,
            container_tracker: dependencies.container_tracker,
            reviewer: None,
            recovery_reasoner: None,
        }
    }

    pub fn with_reviewer(mut self, reviewer: &'a dyn ImplementationReviewer) -> Self {
        self.reviewer = Some(reviewer);
        self
    }

    pub fn with_recovery_reasoner(
        mut self,
        recovery_reasoner: &'a dyn RecoveryReasoner,
    ) -> Self {
        self.recovery_reasoner = Some(recovery_reasoner);
        self
    }

    pub(crate) fn mark_supervisor_blocked(
        &self,
        campaign_id: &str,
        classification: FailureClassification,
        reason: impl Into<String>,
    ) -> Result<CampaignStatus, String> {
        let state = self.require_state(campaign_id)?;
        self.block(state, classification, reason)
    }

    fn bind_container_scope(
        &self,
        campaign_id: &str,
        step_id: Option<&str>,
        action: &str,
    ) -> Option<crate::campaign_container_tracker::CampaignContainerScopeGuard<'_>> {
        self.container_tracker
            .as_ref()
            .map(|tracker| tracker.bind(campaign_id, step_id, action))
    }

    pub fn campaign_dir(&self, campaign_id: &str) -> PathBuf {
        self.state_root
            .join("state")
            .join("campaigns")
            .join(campaign_id)
    }

    pub fn events_path(&self, campaign_id: &str) -> PathBuf {
        self.campaign_dir(campaign_id).join("events.jsonl")
    }

    pub fn state_path(&self, campaign_id: &str) -> PathBuf {
        self.campaign_dir(campaign_id).join("state.json")
    }

    pub fn attempt_dir(&self, campaign_id: &str, step_id: &str, attempt: usize) -> PathBuf {
        self.campaign_dir(campaign_id)
            .join("steps")
            .join(step_id)
            .join(format!("attempt-{attempt}"))
    }

    pub fn load_campaign(&self, campaign_id: &str) -> Result<Campaign, String> {
        let path = self.campaign_dir(campaign_id).join("campaign.json");
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        serde_json::from_str(&content).map_err(|error| error.to_string())
    }

    pub fn load_state(&self, campaign_id: &str) -> Result<Option<CampaignStatus>, String> {
        let path = self.state_path(campaign_id);
        if !path.exists() {
            return Ok(None);
        }
        let campaign_path = self.campaign_dir(campaign_id).join("campaign.json");
        crate::load_campaign_status_compatible(&path, Some(&campaign_path))
    }

    pub fn save_state(&self, state: &CampaignStatus) -> Result<(), String> {
        let _lock = CampaignLock::acquire(&self.campaign_dir(&state.campaign_id))?;
        let merged = self.merge_runner_state_unlocked(state, false)?;
        self.save_state_unlocked(&merged)
    }

    fn save_resumed_state(&self, state: &CampaignStatus) -> Result<(), String> {
        let _lock = CampaignLock::acquire(&self.campaign_dir(&state.campaign_id))?;
        let merged = self.merge_runner_state_unlocked(state, true)?;
        self.save_state_unlocked(&merged)
    }

    /// Runner snapshots are necessarily stale while operator commands run. Preserve all
    /// operator-owned fields and append-only revision state while holding CampaignLock.
    fn merge_runner_state_unlocked(
        &self,
        proposed: &CampaignStatus,
        intentional_resume: bool,
    ) -> Result<CampaignStatus, String> {
        let Some(disk) = self.load_state(&proposed.campaign_id)? else {
            return Ok(proposed.clone());
        };
        let mut merged = proposed.clone();
        merged.cancel_requested |= disk.cancel_requested;
        merged.pause_requested = if intentional_resume {
            false
        } else {
            proposed.pause_requested || disk.pause_requested
        };
        for revision in disk.revisions {
            if !merged.revisions.contains(&revision) {
                merged.revisions.push(revision);
            }
        }
        for step in disk.steps {
            if !merged
                .steps
                .iter()
                .any(|candidate| candidate.step_id == step.step_id)
            {
                merged.steps.push(step);
            }
        }
        if disk.cancel_requested {
            merged.state = CampaignState::Cancelled;
            merged.end_time = disk.end_time;
            merged.blocked_reason = disk.blocked_reason;
            merged.error_message = disk.error_message;
        }
        Ok(merged)
    }

    fn save_state_unlocked(&self, state: &CampaignStatus) -> Result<(), String> {
        let json = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
        atomic_write(&self.state_path(&state.campaign_id), &format!("{json}\n"))
    }

    pub fn log_event(&self, event: CampaignEvent) -> Result<(), String> {
        let line = serde_json::to_string(&event).map_err(|error| error.to_string())?;
        append_line(&self.events_path(&event.campaign_id), &line)
    }

    pub fn validate(&self, campaign: &Campaign) -> Result<(), String> {
        self.validate_live_requirements(campaign)?;
        self.health.assert_workers(
            &campaign.worker_policy.primary,
            &campaign.worker_policy.fallback,
        )?;
        Ok(())
    }

    fn validate_live_requirements(&self, campaign: &Campaign) -> Result<(), String> {
        self.validate_document(campaign)?;
        self.health.assert_executor()?;
        self.workers.runtime(&campaign.worker_policy.primary)?;
        self.workers.runtime(&campaign.worker_policy.fallback)?;
        Ok(())
    }

    fn validate_document(&self, campaign: &Campaign) -> Result<(), String> {
        if campaign.version != "rack-ai/campaign/v1" {
            return Err("unsupported campaign version".to_string());
        }
        if campaign.campaign_id.trim().is_empty()
            || !campaign
                .campaign_id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err("campaign_id cannot be empty".to_string());
        }
        let expected_branch = Campaign::expected_branch(&campaign.campaign_id);
        if campaign.branch != expected_branch {
            return Err(format!("branch must be exactly '{expected_branch}'"));
        }
        if !campaign.allow_local_commits {
            return Err("allow_local_commits must be true".to_string());
        }
        if campaign.permitted_paths.is_empty() {
            return Err("permitted_paths cannot be empty".to_string());
        }
        campaign.validate_permitted_paths()?;
        if campaign.limits.max_runtime_seconds < 60 || campaign.limits.max_runtime_seconds > 172800
        {
            return Err("max_runtime_seconds must be between 60 and 172800".to_string());
        }
        if campaign.limits.max_steps == 0 || campaign.limits.max_steps > 16 {
            return Err("max_steps must be between 1 and 16".to_string());
        }
        if campaign.limits.max_total_attempts == 0 || campaign.limits.max_total_attempts > 32 {
            return Err("max_total_attempts must be between 1 and 32".to_string());
        }
        if campaign.limits.heartbeat_seconds < 10 || campaign.limits.heartbeat_seconds > 60 {
            return Err("heartbeat_seconds must be between 10 and 60".to_string());
        }
        if campaign.limits.network != "disabled" {
            return Err("campaign network must be disabled".to_string());
        }
        if campaign.steps.is_empty() || campaign.steps.len() > campaign.limits.max_steps {
            return Err("campaign step count is outside allowed bounds".to_string());
        }
        let repository_id = RepositoryId::new(campaign.repository.id.clone())?;
        let repository = self.registry.find(&repository_id)?;
        if !repository.enabled() {
            return Err(format!("repository {} is disabled", campaign.repository.id));
        }
        let resolved = self.git.resolve_sha(&ResolveGitShaRequest::new(
            repository.root().to_path_buf(),
            GitRef::new(campaign.repository.base_ref.clone())?,
        ))?;
        if resolved.value() != campaign.repository.base_sha {
            return Err(format!(
                "repository base sha mismatch: resolved {}, expected {}",
                resolved.value(),
                campaign.repository.base_sha
            ));
        }
        for step in &campaign.steps {
            self.validate_step(step)?;
        }
        Ok(())
    }

    fn validate_step(&self, step: &CampaignStep) -> Result<(), String> {
        if step.task.trim().is_empty() {
            return Err(format!("step {} task cannot be empty", step.id));
        }
        if step.allowed_paths.is_empty() {
            return Err(format!("step {} allowed_paths cannot be empty", step.id));
        }
        if step.acceptance.commands.is_empty() {
            return Err(format!(
                "step {} acceptance.commands cannot be empty",
                step.id
            ));
        }
        if step.limits.timeout_seconds == 0 || step.limits.timeout_seconds > 900 {
            return Err(format!(
                "step {} timeout_seconds must be between 1 and 900",
                step.id
            ));
        }
        if step.limits.network != "disabled" {
            return Err(format!("step {} network must be disabled", step.id));
        }
        match step.kind {
            CampaignStepKind::Implementation => {
                if step.required_changed_paths.is_empty() {
                    return Err(format!(
                        "step {} required_changed_paths cannot be empty for implementation steps",
                        step.id
                    ));
                }
            }
            CampaignStepKind::Verification => {
                if !step.required_changed_paths.is_empty() {
                    return Err(format!(
                        "step {} required_changed_paths must be empty for verification steps",
                        step.id
                    ));
                }
            }
        }
        for argv in &step.acceptance.commands {
            let command = AcceptanceCommand::new(argv.clone())?;
            self.command_policy.assert_allowed(&command)?;
        }
        Ok(())
    }

    pub fn start(&self, campaign: &Campaign) -> Result<CampaignStatus, String> {
        self.validate(campaign)?;
        if self.load_state(&campaign.campaign_id)?.is_some() {
            return Err(format!("campaign {} already exists", campaign.campaign_id));
        }
        let repository = self
            .registry
            .find(&RepositoryId::new(campaign.repository.id.clone())?)?;
        let base_sha = GitSha::new(campaign.repository.base_sha.clone())?;
        let workspace_root = self.registry.workspace_root()?;
        let worktree_path = workspace_root
            .join(format!("campaign-{}", campaign.campaign_id).as_str())
            .join("repo");
        self.git.create(
            &CreateChangeWorktreeRequest::new(repository.root().to_path_buf(), base_sha.clone())
                .with_branch_name(campaign.branch.clone())
                .with_worktree_path(worktree_path.clone()),
        )?;
        let dir = self.campaign_dir(&campaign.campaign_id);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let campaign_json =
            serde_json::to_string_pretty(campaign).map_err(|error| error.to_string())?;
        atomic_write(&dir.join("campaign.json"), &format!("{campaign_json}\n"))?;
        let now = self.now_text();
        let state = CampaignStatus {
            schema_version: "rack-ai/campaign/v1".to_string(),
            campaign_id: campaign.campaign_id.clone(),
            campaign_digest: campaign_digest(campaign)?,
            repository_id: campaign.repository.id.clone(),
            base_sha: campaign.repository.base_sha.clone(),
            branch: campaign.branch.clone(),
            worktree_path: worktree_path.to_string_lossy().to_string(),
            current_head_sha: base_sha.value().to_string(),
            state: CampaignState::Running,
            current_step_id: None,
            current_attempt: 0,
            current_worker: None,
            current_action: Some("started".to_string()),
            pause_requested: false,
            cancel_requested: false,
            start_time: now.clone(),
            end_time: None,
            duration_seconds: 0,
            remaining_seconds: campaign.limits.max_runtime_seconds,
            last_heartbeat: now.clone(),
            last_progress_time: Some(now.clone()),
            steps: campaign.steps.iter().map(pending_step).collect(),
            revisions: Vec::new(),
            active_lease_id: None,
            active_container_id: None,
            error_message: None,
            blocked_reason: None,
        };
        self.save_state(&state)?;
        self.emit(
            &campaign.campaign_id,
            None,
            None,
            None,
            "campaign_created",
            "campaign state initialized",
            Some(&state),
        )?;
        Ok(state)
    }

    pub fn mark_detach_setup_failed(
        &self,
        campaign_id: &str,
        reason: &str,
    ) -> Result<CampaignStatus, String> {
        let state = self.require_state(campaign_id)?;
        self.block(
            state,
            FailureClassification::ExecutorUnavailable,
            format!("detached runner setup failed: {reason}"),
        )
    }

    pub fn pause(&self, campaign_id: &str) -> Result<CampaignStatus, String> {
        let _lock = CampaignLock::acquire(&self.campaign_dir(campaign_id))?;
        let mut state = self.require_state(campaign_id)?;

        state.pause_requested = true;

        self.save_state_unlocked(&state)?;
        self.emit(
            campaign_id,
            None,
            None,
            None,
            "campaign_pause_requested",
            "pause requested",
            Some(&state),
        )?;

        Ok(state)
    }

    pub fn cancel(
        &self,
        campaign_id: &str,
        reason: Option<&str>,
    ) -> Result<CampaignStatus, String> {
        let _lock = CampaignLock::acquire(&self.campaign_dir(campaign_id))?;
        let mut state = self.require_state(campaign_id)?;

        state.cancel_requested = true;
        state.pause_requested = false;

        if let Some(reason) = reason {
            state.error_message = Some(reason.to_string());
        }

        if !matches!(
            state.state,
            CampaignState::Completed | CampaignState::Cancelled | CampaignState::Expired
        ) {
            state.state = CampaignState::Cancelled;
            state.end_time = Some(self.now_text());
            state.blocked_reason = Some("operator_cancelled".to_string());
        }

        self.save_state_unlocked(&state)?;
        self.emit(
            campaign_id,
            None,
            None,
            None,
            "campaign_cancelled",
            reason.unwrap_or("cancelled by operator"),
            Some(&state),
        )?;

        Ok(state)
    }

    pub fn revise(
        &self,
        campaign_id: &str,
        revision: CampaignRevisionDocument,
    ) -> Result<CampaignStatus, String> {
        let _lock = CampaignLock::acquire(&self.campaign_dir(campaign_id))?;
        let campaign = self.load_campaign(campaign_id)?;
        let mut state = self.require_state(campaign_id)?;
        if !matches!(state.state, CampaignState::Paused | CampaignState::Blocked) {
            return Err("revise is valid only while paused or blocked".to_string());
        }
        if revision.instruction.trim().is_empty() {
            return Err("revision instruction cannot be empty".to_string());
        }
        if revision.steps.is_empty() {
            return Err("revision must append at least one step".to_string());
        }
        let permitted = &campaign.permitted_paths;

        for step in &revision.steps {
            self.validate_step(step)?;
            assert_step_paths_permitted(
                &step.id,
                &step.allowed_paths,
                &step.required_changed_paths,
                permitted,
            )?;

            if state
                .steps
                .iter()
                .any(|existing| existing.step_id == step.id)
                || campaign.steps.iter().any(|existing| existing.id == step.id)
            {
                return Err(format!("revision step id already exists: {}", step.id));
            }
        }
        let total_steps = state.steps.len() + revision.steps.len();
        if total_steps > campaign.limits.max_steps {
            return Err("revision would exceed max_steps".to_string());
        }
        let added_step_ids = revision
            .steps
            .iter()
            .map(|step| step.id.clone())
            .collect::<Vec<_>>();
        let revision_path = self.campaign_dir(campaign_id).join("revisions.jsonl");
        let revision_json = serde_json::to_string(&revision).map_err(|error| error.to_string())?;
        append_line(&revision_path, &revision_json)?;
        for step in revision.steps {
            state.steps.push(pending_step(&step));
        }
        let now = self.now_text();
        state.revisions.push(RevisionRecord {
            instruction: revision.instruction,
            added_step_ids: added_step_ids.clone(),
            recorded_at: now,
        });
        self.save_state_unlocked(&state)?;
        self.emit(
            campaign_id,
            None,
            None,
            None,
            "campaign_revised",
            format!("appended steps {}", added_step_ids.join(", ")),
            Some(&state),
        )?;
        Ok(state)
    }

    pub fn effective_steps(&self, campaign: &Campaign) -> Result<Vec<CampaignStep>, String> {
        let mut steps = campaign.steps.clone();
        let path = self
            .campaign_dir(&campaign.campaign_id)
            .join("revisions.jsonl");
        if !path.exists() {
            return Ok(steps);
        }
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            let revision: CampaignRevisionDocument =
                serde_json::from_str(line).map_err(|error| error.to_string())?;
            steps.extend(revision.steps);
        }
        Ok(steps)
    }

    pub fn run(&self, campaign_id: &str) -> Result<CampaignStatus, String> {
        self.run_internal(campaign_id, false)
    }

    pub fn resume(&self, campaign_id: &str) -> Result<CampaignStatus, String> {
        let state = self.require_state(campaign_id)?;
        if !matches!(
            state.state,
            CampaignState::Paused | CampaignState::Blocked | CampaignState::Running
        ) {
            return Err(format!("resume is not valid from {:?}", state.state));
        }
        self.run_internal(campaign_id, true)
    }

    pub fn lease_action_timeout(&self, campaign: &Campaign) -> Result<u64, String> {
        Ok(self
            .effective_steps(campaign)?
            .iter()
            .map(|step| step.limits.timeout_seconds)
            .max()
            .unwrap_or(900))
    }

    fn run_internal(&self, campaign_id: &str, resume: bool) -> Result<CampaignStatus, String> {
        let campaign = self.load_campaign(campaign_id)?;
        let mut state = self.require_state(campaign_id)?;
        if resume
            && !matches!(
                state.state,
                CampaignState::Paused | CampaignState::Blocked | CampaignState::Running
            )
        {
            return Err(format!("resume is not valid from {:?}", state.state));
        }
        let now = self.now_text();
        let action_timeout = self.lease_action_timeout(&campaign)?;
        let lease = self.leases.acquire(
            campaign_id,
            &state.repository_id,
            &now,
            campaign.limits.heartbeat_seconds,
            action_timeout,
        )?;
        if resume {
            state.pause_requested = false;
        }
        state.active_lease_id = Some(format!("{}:{}", lease.pid, lease.acquired_at));
        if resume {
            self.save_resumed_state(&state)?;
        } else {
            self.save_state(&state)?;
        }
        let result = self.run_with_lease(&campaign, state, resume);
        let _ = self.leases.release(campaign_id, &campaign.repository.id);
        self.clear_active_lease(campaign_id, result)
    }

    fn clear_active_lease(
        &self,
        campaign_id: &str,
        result: Result<CampaignStatus, String>,
    ) -> Result<CampaignStatus, String> {
        let persist = |state: &mut CampaignStatus| -> Result<(), String> {
            if state.active_lease_id.is_some() {
                state.active_lease_id = None;
                self.save_state(state)?;
            }
            Ok(())
        };
        match result {
            Ok(mut state) => {
                persist(&mut state)?;
                Ok(self.load_state(campaign_id)?.unwrap_or(state))
            }
            Err(error) => {
                if let Ok(Some(mut state)) = self.load_state(campaign_id) {
                    let _ = persist(&mut state);
                }
                Err(error)
            }
        }
    }

    fn run_with_lease(
        &self,
        campaign: &Campaign,
        mut state: CampaignStatus,
        resume: bool,
    ) -> Result<CampaignStatus, String> {
        state = self.recover(campaign, state)?;
        if matches!(
            state.state,
            CampaignState::Completed
                | CampaignState::Cancelled
                | CampaignState::Expired
                | CampaignState::Failed
        ) {
            return Ok(state);
        }
        if state.state == CampaignState::Blocked && !resume {
            return Ok(state);
        }
        if state.cancel_requested {
            state.state = CampaignState::Cancelled;
            return self.finish(state, FailureClassification::OperatorCancelled, "cancelled");
        }
        if state.pause_requested {
            state.state = CampaignState::Paused;
            state.current_action = Some("paused".to_string());
            self.save_state(&state)?;
            self.emit(
                &state.campaign_id,
                None,
                None,
                None,
                "campaign_paused",
                "paused at checkpoint",
                Some(&state),
            )?;
            return Ok(state);
        }
        if let Err(error) = self.preflight_live(campaign, &state) {
            return self.block(state, FailureClassification::from_preflight(&error), error);
        }
        state.state = CampaignState::Running;
        self.heartbeat(&mut state)?;
        self.emit(
            &state.campaign_id,
            None,
            None,
            None,
            "campaign_started",
            "runner active",
            Some(&state),
        )?;
        let steps = self.effective_steps(campaign)?;
        for step in steps {
            let step_index = find_step_index(&state.steps, &step.id)?;
            if state.steps[step_index].disposition == "accepted" {
                continue;
            }
            if let Some(stopped) = self.checkpoint(campaign, &mut state, &step, None)? {
                return Ok(stopped);
            }
            state.current_step_id = Some(step.id.clone());
            state.current_action = Some("step_started".to_string());
            self.save_state(&state)?;
            self.emit(
                &state.campaign_id,
                Some(&step.id),
                None,
                None,
                "step_started",
                format!("starting step {}", step.id),
                Some(&state),
            )?;
            match self.execute_step(campaign, &mut state, &step) {
                Ok(StepOutcome::Accepted) => {
                    self.emit(
                        &state.campaign_id,
                        Some(&step.id),
                        Some(state.steps[step_index].attempts.len()),
                        state.current_worker.as_deref(),
                        "step_accepted",
                        format!("accepted step {}", step.id),
                        Some(&state),
                    )?;
                }
                Ok(StepOutcome::Stopped) => return Ok(state),
                Err(error) => {
                    let classification = if is_executor_error(&error) {
                        FailureClassification::ExecutorUnavailable
                    } else {
                        FailureClassification::ContinuityFailed
                    };
                    return self.block(state, classification, error);
                }
            }
        }
        state.state = CampaignState::Completed;
        state.current_step_id = None;
        state.current_action = Some("completed".to_string());
        state.current_worker = None;
        state.end_time = Some(self.now_text());
        self.refresh_budget(campaign, &mut state);
        self.save_state(&state)?;
        self.emit(
            &state.campaign_id,
            None,
            None,
            None,
            "campaign_completed",
            "campaign completed",
            Some(&state),
        )?;
        Ok(state)
    }

    fn execute_step(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
        step: &CampaignStep,
    ) -> Result<StepOutcome, String> {
        match step.kind {
            CampaignStepKind::Verification => self.execute_verification(campaign, state, step),
            CampaignStepKind::Implementation => self.execute_implementation(campaign, state, step),
        }
    }

    fn execute_verification(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
        step: &CampaignStep,
    ) -> Result<StepOutcome, String> {
        if let Some(stopped) = self.checkpoint(campaign, state, step, None)? {
            return Ok(stopped_from(stopped));
        }
        let attempt_number = total_attempts(state) + 1;
        if attempt_number > campaign.limits.max_total_attempts {
            self.block_in_place(
                state,
                FailureClassification::InadequateImplementation,
                "campaign attempt budget exhausted",
            )?;
            return Ok(StepOutcome::Stopped);
        }
        let start = self.now_text();
        state.current_attempt = attempt_number;
        state.current_worker = Some(campaign.worker_policy.primary.clone());
        state.current_action = Some("acceptance_command".to_string());
        self.heartbeat(state)?;
        let (commands, missing_artifacts, commands_succeeded) =
            match self.run_acceptance(campaign, state, step) {
                Ok(value) => value,
                Err(error) if is_executor_error(&error) => {
                    self.block_in_place(state, FailureClassification::ExecutorUnavailable, error)?;
                    return Ok(StepOutcome::Stopped);
                }
                Err(error) => return Err(error),
            };
        let evidence = self.snapshot_checked(state)?;
        let review = review_attempt(ReviewInput {
            step,
            evidence: &evidence,
            commands_succeeded,
            missing_artifacts,
            implementer_output: None,
            protocol_error: None,
            worker_error: None,
            tool_calls: 0,
            used_host_shell: false,
        });
        self.persist_attempt(
            state,
            step,
            attempt_number,
            AttemptKind::Verification,
            campaign.worker_policy.primary.as_str(),
            &start,
            None,
            None,
            None,
            None,
            &commands,
            &evidence,
            &review,
            None,
        )?;
        if review.disposition == CoordinatorReviewDisposition::Accepted {
            self.mark_step_accepted(state, &step.id, None)?;
            Ok(StepOutcome::Accepted)
        } else {
            self.block_in_place(
                state,
                review
                    .classification
                    .unwrap_or(FailureClassification::AcceptanceFailed),
                review.rationale,
            )?;
            Ok(StepOutcome::Stopped)
        }
    }

    fn execute_implementation(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
        step: &CampaignStep,
    ) -> Result<StepOutcome, String> {
        let mut primary_left = campaign.worker_policy.primary_attempts;
        let mut repair_left = campaign.worker_policy.repair_attempts;
        let mut fallback_left = campaign.worker_policy.fallback_attempts;
        let mut last_review: Option<CoordinatorReview> = None;
        let mut last_evidence_summary = String::new();
        let mut last_commands: Vec<CommandEvidence> = Vec::new();
        let mut last_attempt = 0usize;
        let mut next_instruction: Option<String> = None;
        let mut prefer_fallback_next = false;
        let mut last_failure_fingerprint: Option<String> = None;
        let mut last_repeated_failure_count = 0usize;
        loop {
            if let Some(stopped) = self.checkpoint(campaign, state, step, None)? {
                return Ok(stopped_from(stopped));
            }
            if total_attempts(state) >= campaign.limits.max_total_attempts {
                self.block_in_place(
                    state,
                    FailureClassification::InadequateImplementation,
                    format!("exhausted worker attempts for step {}", step.id),
                )?;
                return Ok(StepOutcome::Stopped);
            }
            let (kind, worker_id, repair_of, fallback_of) = if last_review.is_none() && primary_left > 0 {
                primary_left -= 1;
                (
                    AttemptKind::Primary,
                    campaign.worker_policy.primary.clone(),
                    None,
                    None,
                )
            } else if prefer_fallback_next && fallback_left > 0 {
                fallback_left -= 1;
                prefer_fallback_next = false;
                (
                    AttemptKind::Fallback,
                    campaign.worker_policy.fallback.clone(),
                    None,
                    Some(last_attempt),
                )
            } else if repair_left > 0 {
                repair_left -= 1;
                prefer_fallback_next = false;
                (
                    AttemptKind::Repair,
                    campaign.worker_policy.primary.clone(),
                    Some(last_attempt),
                    None,
                )
            } else if fallback_left > 0 {
                fallback_left -= 1;
                prefer_fallback_next = false;
                (
                    AttemptKind::Fallback,
                    campaign.worker_policy.fallback.clone(),
                    None,
                    Some(last_attempt),
                )
            } else {
                self.block_in_place(
                    state,
                    last_review
                        .as_ref()
                        .and_then(|review| review.classification)
                        .unwrap_or(FailureClassification::InadequateImplementation),
                    format!("exhausted worker attempts for step {}", step.id),
                )?;
                return Ok(StepOutcome::Stopped);
            };
            let attempt_number = total_attempts(state) + 1;
            last_attempt = attempt_number;
            let runtime = self.workers.runtime(&worker_id)?;
            state.current_attempt = attempt_number;
            state.current_worker = Some(runtime.worker_id.clone());
            state.current_action = Some(match kind {
                AttemptKind::Repair => "repair".to_string(),
                AttemptKind::Fallback => "fallback".to_string(),
                _ => "model_request".to_string(),
            });
            self.heartbeat(state)?;
            self.emit(
                &state.campaign_id,
                Some(&step.id),
                Some(attempt_number),
                Some(runtime.worker_id.as_str()),
                match kind {
                    AttemptKind::Repair => "repair_selected",
                    AttemptKind::Fallback => "fallback_selected",
                    _ => "worker_selected",
                },
                format!("selected worker {}", runtime.worker_id),
                Some(state),
            )?;
            let task = match kind {
                AttemptKind::Primary => step.task.clone(),
                _ => next_instruction
                    .clone()
                    .or_else(|| {
                        last_review.as_ref().map(|review| {
                            repair_instruction(step, review, &last_evidence_summary, &last_commands)
                        })
                    })
                    .unwrap_or_else(|| step.task.clone()),
            };
            let launch_instruction = if kind == AttemptKind::Primary {
                None
            } else {
                Some(task.clone())
            };
            if kind != AttemptKind::Primary {
                self.emit(
                    &state.campaign_id,
                    Some(&step.id),
                    Some(attempt_number),
                    Some(runtime.worker_id.as_str()),
                    "recovery_instruction_recorded",
                    "bounded recovery instruction persisted",
                    Some(state),
                )?;
            }
            let start = self.now_text();
            let allowed = allowed_paths(&step.allowed_paths)?;
            let implement_request =
                ImplementChangeRequest::new(PathBuf::from(&state.worktree_path), task)
                    .with_policy(allowed, self.action_timeout_seconds(state, step))
                    .with_max_turns(ChangeLayout::coder_max_turns())
                    .with_worker(
                        runtime.worker_id.clone(),
                        runtime.endpoint.clone(),
                        runtime.api_model_id.clone(),
                    );
            let transport_recovery_started_at = self.clock.now_unix();
            let mut transport_recovery_attempts = 0usize;
            let implement_result = loop {
                self.emit(
                    &state.campaign_id,
                    Some(&step.id),
                    Some(attempt_number),
                    Some(runtime.worker_id.as_str()),
                    "model_request_started",
                    "calling model-backed implementer",
                    Some(state),
                )?;
                let _heartbeat_guard = self.leases.start_background_heartbeat(
                    &state.campaign_id,
                    &state.repository_id,
                    campaign.limits.heartbeat_seconds,
                );
                let _state_heartbeat = self.start_background_state_heartbeat(
                    &state.campaign_id,
                    Some(&step.id),
                    Some(runtime.worker_id.as_str()),
                    "model_request",
                    campaign.limits.heartbeat_seconds,
                );

                let _container_scope =
                    self.bind_container_scope(&state.campaign_id, Some(&step.id), "model_request");
                let implement_result = match self.implementer.implement(&implement_request) {
                    Ok(result) => result,
                    Err(error) => implementer_error_result(error),
                };

                drop(_heartbeat_guard);
                self.emit(
                    &state.campaign_id,
                    Some(&step.id),
                    Some(attempt_number),
                    Some(runtime.worker_id.as_str()),
                    "model_request_completed",
                    "implementer returned",
                    Some(state),
                )?;
                if let Some(error) = implement_result.worker_error() {
                    if is_transient_worker_transport_error(error) {
                        match self.recover_after_transient_model_request_failure(
                            campaign,
                            state,
                            step,
                            runtime.worker_id.as_str(),
                            &mut transport_recovery_attempts,
                            transport_recovery_started_at,
                            error,
                        )? {
                            RecoveryHealthOutcome::Healthy { .. } => continue,
                            RecoveryHealthOutcome::Stopped(stopped) => {
                                return Ok(stopped_from(stopped));
                            }
                        }
                    }
                }
                break implement_result;
            };
            if let Some(stopped) = self.checkpoint(campaign, state, step, None)? {
                self.persist_partial_failure(
                    state,
                    step,
                    attempt_number,
                    kind,
                    &runtime.worker_id,
                    &start,
                    launch_instruction.as_deref(),
                    repair_of,
                    fallback_of,
                    &implement_result,
                    FailureClassification::OperatorCancelled,
                )?;
                return Ok(stopped_from(stopped));
            }
            state.current_action = Some("git_inspect".to_string());
            self.heartbeat(state)?;
            let evidence_after_impl = self.snapshot_checked(state)?;
            self.emit(
                &state.campaign_id,
                Some(&step.id),
                Some(attempt_number),
                Some(runtime.worker_id.as_str()),
                "git_inspection",
                "captured git evidence after implementation",
                Some(state),
            )?;
            state.current_action = Some("acceptance_command".to_string());
            self.heartbeat(state)?;
            let (commands, missing_artifacts, commands_succeeded) =
                if source_paths(evidence_after_impl.changed_paths()).is_empty()
                    && implement_result.protocol_error().is_none()
                    && !looks_protocol(&implement_result)
                {
                    (Vec::new(), Vec::new(), true)
                } else {
                    self.emit(
                        &state.campaign_id,
                        Some(&step.id),
                        Some(attempt_number),
                        Some(runtime.worker_id.as_str()),
                        "acceptance_command_started",
                        "running acceptance commands",
                        Some(state),
                    )?;
                    let result = match self.run_acceptance(campaign, state, step) {
                        Ok(value) => value,
                        Err(error) if is_executor_error(&error) => {
                            self.block_in_place(
                                state,
                                FailureClassification::ExecutorUnavailable,
                                error,
                            )?;
                            return Ok(StepOutcome::Stopped);
                        }
                        Err(error) => return Err(error),
                    };
                    self.emit(
                        &state.campaign_id,
                        Some(&step.id),
                        Some(attempt_number),
                        Some(runtime.worker_id.as_str()),
                        "acceptance_command_completed",
                        "acceptance commands finished",
                        Some(state),
                    )?;
                    result
                };
            let evidence = self.snapshot_checked(state)?;
            last_evidence_summary = evidence.diff_stat().to_string();
            last_commands = commands.clone();
            state.current_action = Some("coordinator_review".to_string());
            self.heartbeat(state)?;
            self.emit(
                &state.campaign_id,
                Some(&step.id),
                Some(attempt_number),
                Some(runtime.worker_id.as_str()),
                "coordinator_review_started",
                "independent coordinator review",
                Some(state),
            )?;
            let mut review = review_attempt(ReviewInput {
                step,
                evidence: &evidence,
                commands_succeeded,
                missing_artifacts,
                implementer_output: Some(implement_result.output()),
                protocol_error: implement_result.protocol_error(),
                worker_error: implement_result.worker_error(),
                tool_calls: implement_result.tool_calls().len(),
                used_host_shell: implement_result.used_host_shell(),
            });
            let mut recovery_trace: Option<RecoveryTrace> = None;
            if review.disposition == CoordinatorReviewDisposition::Accepted {
                if let Some(reviewer) = self.reviewer {
                    let previous_rejection = last_review
                        .as_ref()
                        .map(|previous| previous.rationale.as_str());

                    let _review_heartbeat = self.leases.start_background_heartbeat(
                        &state.campaign_id,
                        &state.repository_id,
                        campaign.limits.heartbeat_seconds,
                    );
                    let _review_state_heartbeat = self.start_background_state_heartbeat(
                        &state.campaign_id,
                        Some(&step.id),
                        Some(runtime.worker_id.as_str()),
                        "coordinator_review",
                        campaign.limits.heartbeat_seconds,
                    );
                    let mut model_request = ModelReviewRequest::from_step(
                        &campaign.campaign_id,
                        step,
                        runtime.worker_id.as_str(),
                        kind == AttemptKind::Fallback,
                        &evidence,
                        &commands,
                        previous_rejection,
                        self.reviewer_timeout_seconds(state, step),
                    );
                    self.add_untracked_review_evidence(state, &mut model_request);
                    let model_review = self.review_with_retries(
                        campaign,
                        state,
                        step,
                        runtime.worker_id.as_str(),
                        attempt_number,
                        reviewer,
                        &model_request,
                    );
                    drop(_review_heartbeat);

                    let review_dir = self.attempt_dir(&state.campaign_id, &step.id, attempt_number);
                    fs::create_dir_all(&review_dir).map_err(|error| error.to_string())?;

                    let model_review_packet = match &model_review {
                        Ok(result) => serde_json::json!({
                            "request": model_request,
                            "prompt": result.prompt,
                            "result": {
                                "disposition": result.disposition,
                                "classification": result.classification,
                                "rationale": result.rationale,
                                "raw_output": result.raw_output,
                                "used_host_shell": result.used_host_shell,
                            }
                        }),
                        Err(error) => serde_json::json!({
                            "request": model_request,
                            "prompt": model_request.prompt(),
                            "result": {
                                "disposition": CoordinatorReviewDisposition::RejectedTerminal,
                                "classification": error.classification,
                                "rationale": error.rationale,
                                "raw_output": null,
                                "used_host_shell": false,
                            }
                        }),
                    };

                    write_json(review_dir.join("model-review.json"), &model_review_packet)?;

                    match model_review {
                        Ok(result) => {
                            review.disposition = result.disposition;
                            review.classification = result.classification;
                            review.rationale = result.rationale;
                        }
                        Err(error) => {
                            review.disposition = CoordinatorReviewDisposition::RejectedTerminal;
                            review.classification = Some(error.classification);
                            review.rationale = error.rationale;
                        }
                    }
                    review.evidence_refs.push("model-review.json".to_string());
                }
                next_instruction = None;
                last_failure_fingerprint = None;
                last_repeated_failure_count = 0;
            }
            if review.disposition == CoordinatorReviewDisposition::RejectedRetryable {
                let planning = self.plan_retry_from_failure(
                    campaign,
                    state,
                    step,
                    attempt_number,
                    runtime.worker_id.as_str(),
                    launch_instruction.as_deref(),
                    &review,
                    &evidence,
                    &commands,
                    &implement_result,
                    last_failure_fingerprint.as_deref(),
                    last_repeated_failure_count,
                    repair_left + fallback_left,
                    fallback_left,
                )?;
                review = planning.review;
                next_instruction = planning.next_instruction.clone();
                prefer_fallback_next = planning.prefer_fallback_worker;
                last_failure_fingerprint = Some(planning.fingerprint);
                last_repeated_failure_count = planning.repeated_failure_count;
                recovery_trace = planning.trace;
            }
            if review.disposition == CoordinatorReviewDisposition::RejectedRetryable {
                if let Some(classification) = review.classification {
                    if !classification.retryable() {
                        review.disposition = CoordinatorReviewDisposition::RejectedTerminal;
                    }
                }
            }
            last_review = Some(review.clone());
            let mut commit_sha = None;
            if review.disposition == CoordinatorReviewDisposition::Accepted {
                if let Some(stopped) = self.checkpoint(campaign, state, step, None)? {
                    self.persist_attempt(
                        state,
                        step,
                        attempt_number,
                        kind,
                        runtime.worker_id.as_str(),
                        &start,
                        launch_instruction.as_deref(),
                        repair_of,
                        fallback_of,
                        Some(&implement_result),
                        &commands,
                        &evidence,
                        &review,
                        None,
                    )?;
                    return Ok(stopped_from(stopped));
                }
                state.current_action = Some("pre_commit_inspect".to_string());
                self.heartbeat(state)?;
                let pre_commit = self.snapshot_checked(state)?;
                let pre_commit_review = review_attempt(ReviewInput {
                    step,
                    evidence: &pre_commit,
                    commands_succeeded: true,
                    missing_artifacts: Vec::new(),
                    implementer_output: Some(implement_result.output()),
                    protocol_error: implement_result.protocol_error(),
                    worker_error: implement_result.worker_error(),
                    tool_calls: implement_result.tool_calls().len(),
                    used_host_shell: implement_result.used_host_shell(),
                });
                if pre_commit_review.disposition != CoordinatorReviewDisposition::Accepted {
                    if let Some(stopped) = self.checkpoint(campaign, state, step, Some(attempt_number))? {
                        return Ok(stopped_from(stopped));
                    }
                    self.persist_attempt(
                        state,
                        step,
                        attempt_number,
                        kind,
                        runtime.worker_id.as_str(),
                        &start,
                        launch_instruction.as_deref(),
                        repair_of,
                        fallback_of,
                        Some(&implement_result),
                        &commands,
                        &pre_commit,
                        &pre_commit_review,
                        None,
                    )?;
                    self.block_in_place(
                        state,
                        pre_commit_review
                            .classification
                            .unwrap_or(FailureClassification::PathPolicyFailed),
                        pre_commit_review.rationale,
                    )?;
                    return Ok(StepOutcome::Stopped);
                }
                state.current_action = Some("git_commit".to_string());
                self.heartbeat(state)?;
                let changed = source_paths(pre_commit.changed_paths());
                commit_sha = Some(
                    self.git
                        .commit_local(&CampaignCommitRequest::new(
                            Path::new(&state.worktree_path).to_path_buf(),
                            &campaign.campaign_id,
                            &step.id,
                            changed.clone(),
                        ))?
                        .value()
                        .to_string(),
                );
                let head = self.git.current_head(Path::new(&state.worktree_path))?;
                state.current_head_sha = head.value().to_string();
                self.emit(
                    &state.campaign_id,
                    Some(&step.id),
                    Some(attempt_number),
                    Some(runtime.worker_id.as_str()),
                    "step_accepted",
                    format!("accepted step {}", step.id),
                    Some(state),
                )?;
            }
            self.persist_attempt(
                state,
                step,
                attempt_number,
                kind,
                runtime.worker_id.as_str(),
                &start,
                launch_instruction.as_deref(),
                repair_of,
                fallback_of,
                Some(&implement_result),
                &commands,
                &evidence,
                &review,
                commit_sha.clone(),
            )?;
            if let Some(trace) = recovery_trace.as_ref() {
                self.persist_recovery_trace(&state.campaign_id, &step.id, attempt_number, trace)?;
            }
            match review.disposition {
                CoordinatorReviewDisposition::Accepted => {
                    if commit_sha.is_none() && matches!(step.kind, CampaignStepKind::Implementation)
                    {
                        self.block_in_place(
                            state,
                            FailureClassification::ContinuityFailed,
                            "accepted review did not produce a local commit",
                        )?;
                        return Ok(StepOutcome::Stopped);
                    }
                    self.mark_step_accepted(state, &step.id, commit_sha)?;
                    return Ok(StepOutcome::Accepted);
                }
                CoordinatorReviewDisposition::RejectedTerminal => {
                    self.block_in_place(
                        state,
                        review
                            .classification
                            .unwrap_or(FailureClassification::PathPolicyFailed),
                        review.rationale,
                    )?;
                    return Ok(StepOutcome::Stopped);
                }
                CoordinatorReviewDisposition::RejectedRetryable => {
                    let classification = review
                        .classification
                        .unwrap_or(FailureClassification::InadequateImplementation);
                    if !classification.retryable()
                        || (primary_left == 0 && repair_left == 0 && fallback_left == 0)
                    {
                        self.block_in_place(state, classification, review.rationale)?;
                        return Ok(StepOutcome::Stopped);
                    }
                    self.reset_rejected_attempt(state, step, attempt_number, &evidence)?;
                }
            }
        }
    }

    fn plan_retry_from_failure(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
        step: &CampaignStep,
        attempt_number: usize,
        worker_id: &str,
        launch_instruction: Option<&str>,
        review: &CoordinatorReview,
        evidence: &GitEvidence,
        commands: &[CommandEvidence],
        implement_result: &ImplementChangeResult,
        last_failure_fingerprint: Option<&str>,
        last_repeated_failure_count: usize,
        remaining_attempt_budget: usize,
        fallback_left: usize,
    ) -> Result<RecoveryPlanningOutcome, String> {
        let classification = review
            .classification
            .unwrap_or(FailureClassification::InadequateImplementation);
        let fingerprint = self.failure_fingerprint(classification, evidence, commands, implement_result);
        let repeated_failure_count = if last_failure_fingerprint == Some(fingerprint.as_str()) {
            last_repeated_failure_count + 1
        } else {
            0
        };
        let mut planned_review = review.clone();
        let default_instruction = repair_instruction(step, &planned_review, evidence.diff_stat(), commands);
        if !should_diagnose_retryable_failure(classification, repeated_failure_count) {
            planned_review.repair_instruction = Some(default_instruction.clone());
            return Ok(RecoveryPlanningOutcome {
                review: planned_review,
                next_instruction: Some(default_instruction),
                prefer_fallback_worker: false,
                trace: None,
                fingerprint,
                repeated_failure_count,
            });
        }
        let Some(reasoner) = self.recovery_reasoner else {
            planned_review.repair_instruction = Some(default_instruction.clone());
            return Ok(RecoveryPlanningOutcome {
                review: planned_review,
                next_instruction: Some(default_instruction),
                prefer_fallback_worker: false,
                trace: None,
                fingerprint,
                repeated_failure_count,
            });
        };
        state.current_action = Some("recovery_diagnosis".to_string());
        self.heartbeat(state)?;
        let request = RecoveryReasoningRequest::new(
            self.build_recovery_context(
                campaign,
                state,
                step,
                worker_id,
                launch_instruction,
                &planned_review,
                evidence,
                commands,
                implement_result,
                classification,
                fingerprint.clone(),
                repeated_failure_count,
                remaining_attempt_budget,
            ),
            self.reviewer_timeout_seconds(state, step),
        );
        self.emit(
            &state.campaign_id,
            Some(&step.id),
            Some(attempt_number),
            Some(worker_id),
            "recovery_diagnosis_started",
            "local-primary recovery diagnosis",
            Some(state),
        )?;
        let diagnosis = reasoner.diagnose(&request);
        let mut trace = RecoveryTrace {
            request: request.clone(),
            result: diagnosis.clone(),
            fingerprint: fingerprint.clone(),
            repeated_failure_count,
        };
        let mut prefer_fallback_worker = false;
        match diagnosis {
            Ok(result) => {
                let mut decision = result.decision.clone();
                if repeated_failure_count > 0
                    && decision.kind == RecoveryDecisionKind::Repair
                    && decision.worker_action == RecoveryWorkerAction::SameWorker
                {
                    decision = self.force_stagnation_replan(step, review, commands, evidence);
                    trace.result = Ok(RecoveryReasoningResult {
                        decision: decision.clone(),
                        prompt: result.prompt,
                        raw_output: result.raw_output,
                    });
                }
                prefer_fallback_worker =
                    decision.worker_action == RecoveryWorkerAction::FallbackWorker;
                if prefer_fallback_worker && fallback_left == 0 {
                    planned_review.disposition = CoordinatorReviewDisposition::RejectedTerminal;
                    planned_review.classification = Some(FailureClassification::RecoveryFailure);
                    planned_review.rationale =
                        "recovery requested fallback worker but fallback budget is exhausted"
                            .to_string();
                    planned_review.repair_instruction = None;
                } else {
                    match decision.kind {
                        RecoveryDecisionKind::Repair
                        | RecoveryDecisionKind::Replan
                        | RecoveryDecisionKind::RetryTransient => {
                            let instruction = decision.next_instruction.clone().unwrap_or_else(|| {
                                self.default_recovery_instruction(
                                    step,
                                    &planned_review,
                                    evidence,
                                    commands,
                                    &decision,
                                )
                            });
                            planned_review.repair_instruction = Some(instruction);
                        }
                        RecoveryDecisionKind::BlockInsufficientAuthority => {
                            planned_review.disposition = CoordinatorReviewDisposition::RejectedTerminal;
                            planned_review.classification = Some(FailureClassification::InsufficientAuthority);
                            planned_review.rationale = decision.rationale.clone();
                            planned_review.repair_instruction = None;
                        }
                        RecoveryDecisionKind::BlockTerminal => {
                            planned_review.disposition = CoordinatorReviewDisposition::RejectedTerminal;
                            planned_review.classification = Some(FailureClassification::RecoveryFailure);
                            planned_review.rationale = decision.rationale.clone();
                            planned_review.repair_instruction = None;
                        }
                    }
                }
                self.emit(
                    &state.campaign_id,
                    Some(&step.id),
                    Some(attempt_number),
                    Some(worker_id),
                    "recovery_decision_recorded",
                    format!(
                        "recovery decision {}: {}",
                        recovery_decision_name(decision.kind),
                        decision.rationale
                    ),
                    Some(state),
                )?;
            }
            Err(error) => {
                planned_review.disposition = CoordinatorReviewDisposition::RejectedTerminal;
                planned_review.classification = Some(FailureClassification::RecoveryFailure);
                planned_review.rationale = format!("recovery diagnosis failed closed: {error}");
                planned_review.repair_instruction = None;
                self.emit(
                    &state.campaign_id,
                    Some(&step.id),
                    Some(attempt_number),
                    Some(worker_id),
                    "recovery_decision_failed",
                    error,
                    Some(state),
                )?;
            }
        }
        let next_instruction = planned_review.repair_instruction.clone();
        Ok(RecoveryPlanningOutcome {
            review: planned_review,
            next_instruction,
            prefer_fallback_worker,
            trace: Some(trace),
            fingerprint,
            repeated_failure_count,
        })
    }

    fn persist_partial_failure(
        &self,
        state: &mut CampaignStatus,
        step: &CampaignStep,
        attempt: usize,
        kind: AttemptKind,
        worker_id: &str,
        start: &str,
        launch_instruction: Option<&str>,
        repair_of: Option<usize>,
        fallback_of: Option<usize>,
        result: &ImplementChangeResult,
        classification: FailureClassification,
    ) -> Result<(), String> {
        let evidence = self
            .git
            .snapshot(std::path::Path::new(&state.worktree_path))
            .ok();
        let review = CoordinatorReview {
            disposition: CoordinatorReviewDisposition::RejectedTerminal,
            classification: Some(classification),
            rationale: classification.as_str().to_string(),
            evidence_refs: vec!["worker-transcript.json".to_string()],
            repair_instruction: None,
        };
        self.persist_attempt(
            state,
            step,
            attempt,
            kind,
            worker_id,
            start,
            launch_instruction,
            repair_of,
            fallback_of,
            Some(result),
            &[],
            evidence
                .as_ref()
                .unwrap_or(&empty_evidence(&state.current_head_sha)?),
            &review,
            None,
        )
    }

    fn persist_attempt(
        &self,
        state: &mut CampaignStatus,
        step: &CampaignStep,
        attempt: usize,
        kind: AttemptKind,
        worker_id: &str,
        start: &str,
        launch_instruction: Option<&str>,
        repair_of: Option<usize>,
        fallback_of: Option<usize>,
        implement_result: Option<&ImplementChangeResult>,
        commands: &[CommandEvidence],
        evidence: &GitEvidence,
        review: &CoordinatorReview,
        commit_sha: Option<String>,
    ) -> Result<(), String> {
        let dir = self.attempt_dir(&state.campaign_id, &step.id, attempt);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let launch_instruction = launch_instruction.map(str::to_string);
        let next_repair_instruction = review.repair_instruction.clone();
        let mut packet_review = review.clone();
        packet_review.repair_instruction = launch_instruction.clone();
        let packet = ReviewPacketDocument {
            step_id: step.id.clone(),
            attempt,
            worker_id: worker_id.to_string(),
            attempt_kind: kind,
            repair_instruction: launch_instruction.clone(),
            next_repair_instruction: next_repair_instruction.clone(),
            review: packet_review,
            changed_paths: source_paths(evidence.changed_paths()),
            commit_sha: commit_sha.clone(),
        };
        write_json(dir.join("review-packet.json"), &packet)?;
        let transcript = serde_json::json!({
            "worker_id": worker_id,
            "attempt_kind": kind,
            "output": implement_result.map(|item| item.output().to_string()),
            "protocol_error": implement_result.and_then(|item| item.protocol_error().map(|value| value.to_string())),
            "worker_error": implement_result.and_then(|item| item.worker_error().map(|value| value.to_string())),
            "executor_kind": implement_result.map(|item| item.executor_kind().to_string()),
            "used_host_shell": implement_result.map(|item| item.used_host_shell()).unwrap_or(false),
            "tool_calls": implement_result.map(|item| item.tool_calls().iter().map(|call| serde_json::json!({
                "name": call.name,
                "arguments": call.arguments,
                "result": call.result,
            })).collect::<Vec<_>>()).unwrap_or_default(),
            "repair_instruction": launch_instruction,
            "next_repair_instruction": next_repair_instruction,
            "repair_of": repair_of,
            "fallback_of": fallback_of,
        });
        write_json(dir.join("worker-transcript.json"), &transcript)?;
        write_json(dir.join("command-evidence.json"), &commands)?;
        write_json(
            dir.join("git-evidence.json"),
            &GitEvidenceDocument {
                head_sha: evidence.head_sha().value().to_string(),
                status: evidence.status().to_string(),
                diff: evidence.diff().to_string(),
                diff_stat: evidence.diff_stat().to_string(),
                changed_paths: evidence.changed_paths().to_vec(),
            },
        )?;
        let record = StepAttemptRecord {
            attempt,
            worker_id: worker_id.to_string(),
            kind,
            start_time: start.to_string(),
            end_time: self.now_text(),
            disposition: review.disposition,
            classification: review.classification,
            rationale: review.rationale.clone(),
            commit_sha,
            repair_instruction: launch_instruction,
            next_repair_instruction,
            repair_of,
            fallback_of,
        };
        let step_index = find_step_index(&state.steps, &step.id)?;
        state.steps[step_index].review_disposition = Some(review.disposition);
        state.steps[step_index].review_rationale = Some(review.rationale.clone());
        state.steps[step_index].attempts.push(record);
        if review.disposition != CoordinatorReviewDisposition::Accepted {
            state.steps[step_index].disposition = match review.disposition {
                CoordinatorReviewDisposition::RejectedTerminal => "blocked".to_string(),
                _ => "rejected".to_string(),
            };
        }
        self.save_state(state)?;
        let event_type = match review.disposition {
            CoordinatorReviewDisposition::Accepted => "coordinator_review_accepted",
            _ => "coordinator_review_rejected",
        };
        self.emit(
            &state.campaign_id,
            Some(&step.id),
            Some(attempt),
            Some(worker_id),
            event_type,
            review.rationale.clone(),
            Some(state),
        )?;
        Ok(())
    }

    fn mark_step_accepted(
        &self,
        state: &mut CampaignStatus,
        step_id: &str,
        commit_sha: Option<String>,
    ) -> Result<(), String> {
        let step_index = find_step_index(&state.steps, step_id)?;
        state.steps[step_index].disposition = "accepted".to_string();
        state.steps[step_index].accepted_commit = commit_sha;
        state.last_progress_time = Some(self.now_text());
        self.save_state(state)
    }

    fn run_acceptance(
        &self,
        campaign: &Campaign,
        state: &CampaignStatus,
        step: &CampaignStep,
    ) -> Result<(Vec<CommandEvidence>, Vec<String>, bool), String> {
        let timeout = self.action_timeout_seconds(state, step);
        let _heartbeat_guard = self.leases.start_background_heartbeat(
            &state.campaign_id,
            &state.repository_id,
            campaign.limits.heartbeat_seconds,
        );
        let _state_heartbeat = self.start_background_state_heartbeat(
            &state.campaign_id,
            Some(&step.id),
            state.current_worker.as_deref(),
            "acceptance_command",
            campaign.limits.heartbeat_seconds,
        );
        let _container_scope =
            self.bind_container_scope(&state.campaign_id, Some(&step.id), "acceptance_command");
        let mut commands = Vec::new();
        let mut succeeded = true;
        for argv in &step.acceptance.commands {
            let command = AcceptanceCommand::new(argv.clone())?;
            let result = self.executor.run_command(
                &RunCommandRequest::new(
                    PathBuf::from(&state.worktree_path),
                    command.argv().to_vec(),
                )?
                .with_timeout_seconds(timeout),
            );
            match result {
                Ok(execution) => {
                    if !execution.evidence().succeeded() {
                        succeeded = false;
                    }
                    commands.push(execution.evidence().clone());
                }
                Err(error) => {
                    if is_executor_error(&error) {
                        return Err(error);
                    }
                    succeeded = false;
                    commands.push(CommandEvidence::new(argv.clone(), 1).with_stderr(error));
                }
            }
        }
        let mut missing = Vec::new();
        for artifact in &step.acceptance.required_artifacts {
            match self.executor.read_file(&ReadFileRequest::new(
                PathBuf::from(&state.worktree_path),
                WorkspacePath::parse(artifact)?,
            )) {
                Ok(_) => {}
                Err(error) if is_executor_error(&error) => return Err(error),
                Err(_) => missing.push(artifact.clone()),
            }
        }
        let succeeded = succeeded && missing.is_empty();
        Ok((commands, missing, succeeded))
    }

    fn action_timeout_seconds(&self, state: &CampaignStatus, step: &CampaignStep) -> u32 {
        step.limits
            .timeout_seconds
            .min(state.remaining_seconds.max(1))
            .min(u64::from(u32::MAX)) as u32
    }

    fn reset_rejected_attempt(
        &self,
        state: &mut CampaignStatus,
        step: &CampaignStep,
        attempt_number: usize,
        evidence: &GitEvidence,
    ) -> Result<(), String> {
        if evidence.changed_paths().is_empty() {
            return Ok(());
        }
        let head = GitSha::new(state.current_head_sha.clone())?;
        self.git
            .reset_managed_worktree(
                Path::new(&state.worktree_path),
                &head,
                evidence.changed_paths(),
            )
            .map_err(|error| format!("failed to reset rejected attempt worktree: {error}"))?;
        self.emit(
            &state.campaign_id,
            Some(&step.id),
            Some(attempt_number),
            state.current_worker.as_deref(),
            "rejected_attempt_reset",
            "reset rejected worktree changes back to the last accepted head",
            Some(state),
        )?;
        Ok(())
    }

    fn persist_recovery_trace(
        &self,
        campaign_id: &str,
        step_id: &str,
        attempt: usize,
        trace: &RecoveryTrace,
    ) -> Result<(), String> {
        let dir = self.attempt_dir(campaign_id, step_id, attempt);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let packet = match &trace.result {
            Ok(result) => serde_json::json!({
                "request": trace.request.context(),
                "prompt": result.prompt,
                "fingerprint": trace.fingerprint,
                "repeated_failure_count": trace.repeated_failure_count,
                "result": {
                    "decision": result.decision,
                    "raw_output": result.raw_output,
                }
            }),
            Err(error) => serde_json::json!({
                "request": trace.request.context(),
                "prompt": trace.request.prompt(),
                "fingerprint": trace.fingerprint,
                "repeated_failure_count": trace.repeated_failure_count,
                "result": {
                    "error": error,
                }
            }),
        };
        write_json(dir.join("recovery-decision.json"), &packet)
    }

    fn build_recovery_context(
        &self,
        campaign: &Campaign,
        state: &CampaignStatus,
        step: &CampaignStep,
        _worker_id: &str,
        _launch_instruction: Option<&str>,
        review: &CoordinatorReview,
        evidence: &GitEvidence,
        commands: &[CommandEvidence],
        implement_result: &ImplementChangeResult,
        classification: FailureClassification,
        fingerprint: String,
        repeated_failure_count: usize,
        remaining_attempt_budget: usize,
    ) -> RecoveryContext {
        let step_index = find_step_index(&state.steps, &step.id).unwrap_or(0);
        let previous_attempts = state.steps[step_index]
            .attempts
            .iter()
            .map(|attempt| RecoveryAttemptSummary {
                attempt: attempt.attempt,
                worker_id: attempt.worker_id.clone(),
                attempt_kind: attempt_kind_name(attempt.kind).to_string(),
                classification: attempt.classification,
                rationale: bounded_chars(&attempt.rationale, 240),
                launch_instruction: attempt.repair_instruction.clone(),
                next_instruction: attempt.next_repair_instruction.clone(),
                recovery_decision: self.load_attempt_recovery_decision(
                    &state.campaign_id,
                    &step.id,
                    attempt.attempt,
                ),
                fingerprint: self.load_attempt_recovery_fingerprint(
                    &state.campaign_id,
                    &step.id,
                    attempt.attempt,
                ),
            })
            .collect();
        RecoveryContext {
            campaign_id: state.campaign_id.clone(),
            step_id: step.id.clone(),
            original_task: step.task.clone(),
            campaign_permitted_paths: campaign.permitted_paths.clone(),
            allowed_paths: step.allowed_paths.clone(),
            required_changed_paths: step.required_changed_paths.clone(),
            acceptance_commands: step.acceptance.commands.clone(),
            changed_paths: source_paths(evidence.changed_paths()),
            git_status: bounded_chars(evidence.status(), 800),
            diff_stat: bounded_chars(evidence.diff_stat(), 800),
            diff_excerpt: bounded_chars(evidence.diff(), 1600),
            failure_classification: classification,
            failure_rationale: bounded_chars(&review.rationale, 320),
            command_failure: command_failure(commands),
            tool_attempts: tool_attempts(step, implement_result),
            previous_attempts,
            repeated_failure_count,
            current_fingerprint: fingerprint,
            remaining_attempt_budget,
        }
    }

    fn load_attempt_recovery_decision(
        &self,
        campaign_id: &str,
        step_id: &str,
        attempt: usize,
    ) -> Option<RecoveryDecisionKind> {
        let value = self.load_recovery_trace_value(campaign_id, step_id, attempt)?;
        serde_json::from_value(value.get("result")?.get("decision")?.get("kind")?.clone()).ok()
    }

    fn load_attempt_recovery_fingerprint(
        &self,
        campaign_id: &str,
        step_id: &str,
        attempt: usize,
    ) -> Option<String> {
        self.load_recovery_trace_value(campaign_id, step_id, attempt)?
            .get("fingerprint")?
            .as_str()
            .map(str::to_string)
    }

    fn load_recovery_trace_value(
        &self,
        campaign_id: &str,
        step_id: &str,
        attempt: usize,
    ) -> Option<serde_json::Value> {
        let path = self
            .attempt_dir(campaign_id, step_id, attempt)
            .join("recovery-decision.json");
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn failure_fingerprint(
        &self,
        classification: FailureClassification,
        evidence: &GitEvidence,
        commands: &[CommandEvidence],
        implement_result: &ImplementChangeResult,
    ) -> String {
        let changed = source_paths(evidence.changed_paths()).join(",");
        let command = command_failure(commands)
            .map(|failure| format!(
                "{}:{}:{}",
                failure.command,
                failure.exit_code,
                first_non_empty_line(&failure.stderr_excerpt)
            ))
            .unwrap_or_else(|| "no-command-failure".to_string());
        let forbidden = tool_attempts_for_fingerprint(implement_result);
        format!(
            "{}|{}|{}|{}",
            classification.as_str(),
            changed,
            command,
            forbidden.join(",")
        )
    }

    fn default_recovery_instruction(
        &self,
        step: &CampaignStep,
        review: &CoordinatorReview,
        evidence: &GitEvidence,
        commands: &[CommandEvidence],
        decision: &RecoveryDecision,
    ) -> String {
        let base = repair_instruction(step, review, evidence.diff_stat(), commands);
        match decision.kind {
            RecoveryDecisionKind::Replan => format!(
                "Replan the implementation for step {}. Preserve existing callers and behavior outside allowed_paths, and revise the implementation strategy only within allowed_paths.
Diagnosis: {}
{}",
                step.id, decision.rationale, base
            ),
            _ => base,
        }
    }

    fn force_stagnation_replan(
        &self,
        step: &CampaignStep,
        review: &CoordinatorReview,
        commands: &[CommandEvidence],
        evidence: &GitEvidence,
    ) -> RecoveryDecision {
        RecoveryDecision {
            kind: RecoveryDecisionKind::Replan,
            failure_kind: RecoveryFailureKind::RepeatedFailure,
            rationale: "repeated equivalent failure requires a strategy change instead of another same-strategy repair".to_string(),
            evidence_refs: vec![
                "git-evidence.json".to_string(),
                "command-evidence.json".to_string(),
                "worker-transcript.json".to_string(),
            ],
            constraint_conflict: true,
            same_strategy_viable: false,
            worker_action: RecoveryWorkerAction::FallbackWorker,
            next_instruction: Some(format!(
                "Replan the implementation for step {}. The previous strategy repeated the same failure. Preserve out-of-scope callers and modify only allowed_paths.
{}",
                step.id,
                repair_instruction(step, review, evidence.diff_stat(), commands)
            )),
            insufficient_authority: false,
            stagnation_detected: true,
        }
    }

    fn reviewer_timeout_seconds(&self, state: &CampaignStatus, step: &CampaignStep) -> u32 {
        let remaining = state.remaining_seconds.max(1).min(u64::from(u32::MAX)) as u32;
        self.action_timeout_seconds(state, step)
            .max(REVIEWER_MIN_TIMEOUT_SECONDS)
            .min(remaining)
    }

    fn review_with_retries(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
        step: &CampaignStep,
        worker_id: &str,
        attempt_number: usize,
        reviewer: &dyn ImplementationReviewer,
        request: &ModelReviewRequest,
    ) -> Result<crate::ModelReviewResult, ReviewerFailure> {
        let mut attempts = 0usize;
        loop {
            attempts += 1;
            match reviewer.review(request) {
                Ok(result) => return Ok(result),
                Err(error) => {
                    let classification = classify_reviewer_error(&error);
                    if classification != FailureClassification::ReviewerTimeout
                        || attempts >= REVIEWER_MAX_ATTEMPTS
                    {
                        return Err(ReviewerFailure {
                            classification,
                            rationale: format!("model reviewer failed closed: {error}"),
                        });
                    }
                    state.current_action = Some("coordinator_review_retry_wait".to_string());
                    state.error_message = Some(format!(
                        "model reviewer transient failure after attempt {attempts}/{}: {error}",
                        REVIEWER_MAX_ATTEMPTS
                    ));
                    self.save_state(state)
                        .map_err(|save_error| ReviewerFailure {
                            classification: FailureClassification::ReviewerFailure,
                            rationale: save_error,
                        })?;
                    self.emit(
                        &state.campaign_id,
                        Some(&step.id),
                        Some(attempt_number),
                        Some(worker_id),
                        "coordinator_review_retrying",
                        format!(
                            "model reviewer transient failure after attempt {attempts}/{}: {error}",
                            REVIEWER_MAX_ATTEMPTS
                        ),
                        Some(state),
                    )
                    .map_err(|emit_error| ReviewerFailure {
                        classification: FailureClassification::ReviewerFailure,
                        rationale: emit_error,
                    })?;
                    if self
                        .recovery_checkpoint(campaign, state)
                        .map_err(|checkpoint_error| ReviewerFailure {
                            classification: FailureClassification::ReviewerFailure,
                            rationale: checkpoint_error,
                        })?
                        .is_some()
                    {
                        return Err(ReviewerFailure {
                            classification: FailureClassification::ReviewerFailure,
                            rationale: "review interrupted by operator checkpoint".to_string(),
                        });
                    }
                    let delay = recovery_delay_seconds(&REVIEWER_RETRY_DELAYS_SECONDS, attempts);
                    self.sleeper.sleep_seconds(delay);
                    if self
                        .recovery_checkpoint(campaign, state)
                        .map_err(|checkpoint_error| ReviewerFailure {
                            classification: FailureClassification::ReviewerFailure,
                            rationale: checkpoint_error,
                        })?
                        .is_some()
                    {
                        return Err(ReviewerFailure {
                            classification: FailureClassification::ReviewerFailure,
                            rationale: "review interrupted by operator checkpoint".to_string(),
                        });
                    }
                    state.current_action = Some("coordinator_review".to_string());
                    state.error_message = None;
                    self.save_state(state)
                        .map_err(|save_error| ReviewerFailure {
                            classification: FailureClassification::ReviewerFailure,
                            rationale: save_error,
                        })?;
                }
            }
        }
    }

    /// `git diff` omits untracked files. Read them through the workspace executor so the
    /// read-only reviewer sees the actual implementation without gaining host filesystem tools.
    fn add_untracked_review_evidence(
        &self,
        state: &CampaignStatus,
        request: &mut ModelReviewRequest,
    ) {
        let untracked = request
            .git_status
            .lines()
            .filter_map(|line| line.strip_prefix("?? "))
            .collect::<Vec<_>>();
        for path in untracked {
            let Ok(workspace_path) = WorkspacePath::parse(path) else {
                continue;
            };
            let read = ReadFileRequest::new(PathBuf::from(&state.worktree_path), workspace_path)
                .with_timeout_seconds(request.timeout_seconds);
            if let Ok(result) = self.executor.read_file(&read) {
                request.diff.push_str(&format!(
                    "\n--- /dev/null\n+++ b/{path}\n{}",
                    result.content()
                ));
                request.diff_stat.push_str(&format!("\n{path} | new file"));
            }
        }
    }

    fn start_background_state_heartbeat(
        &self,
        campaign_id: &str,
        step_id: Option<&str>,
        worker_id: Option<&str>,
        action: &str,
        heartbeat_seconds: u64,
    ) -> StateHeartbeatGuard {
        let state_root = self.state_root.clone();
        let campaign_id = campaign_id.to_string();
        let step_id = step_id.map(str::to_string);
        let worker_id = worker_id.map(str::to_string);
        let action = action.to_string();
        let interval = Duration::from_secs(heartbeat_seconds.clamp(1, 30));
        let (stop_tx, stop_rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            loop {
                match stop_rx.recv_timeout(interval) {
                    Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if update_background_state_heartbeat(
                            &state_root,
                            &campaign_id,
                            step_id.as_deref(),
                            worker_id.as_deref(),
                            &action,
                        )
                        .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        StateHeartbeatGuard {
            stop: Some(stop_tx),
            thread: Some(thread),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_background_state_heartbeat(
        &self,
        campaign_id: &str,
        step_id: Option<&str>,
        worker_id: Option<&str>,
        action: &str,
    ) -> Result<(), String> {
        update_background_state_heartbeat(&self.state_root, campaign_id, step_id, worker_id, action)
    }

    fn snapshot_checked(&self, state: &CampaignStatus) -> Result<GitEvidence, String> {
        let evidence = self
            .git
            .snapshot(std::path::Path::new(&state.worktree_path))?;
        if evidence.head_sha().value() != state.current_head_sha {
            return Err(format!(
                "worktree HEAD {} does not match recorded HEAD {}",
                evidence.head_sha().value(),
                state.current_head_sha
            ));
        }
        Ok(evidence)
    }

    fn recover(
        &self,
        campaign: &Campaign,
        mut state: CampaignStatus,
    ) -> Result<CampaignStatus, String> {
        let digest = campaign_digest(campaign)?;
        if digest != state.campaign_digest {
            return self.block(
                state,
                FailureClassification::ContinuityFailed,
                "campaign digest mismatch",
            );
        }
        if campaign.repository.base_sha != state.base_sha {
            return self.block(
                state,
                FailureClassification::ContinuityFailed,
                "base SHA mismatch",
            );
        }
        if campaign.branch != state.branch {
            return self.block(
                state,
                FailureClassification::ContinuityFailed,
                "branch mismatch",
            );
        }
        let worktree = std::path::PathBuf::from(&state.worktree_path);
        if !worktree.exists() {
            return self.block(
                state,
                FailureClassification::ContinuityFailed,
                "worktree is missing",
            );
        }
        let branch = self.git.current_branch(&worktree)?;
        if branch != state.branch {
            return self.block(
                state,
                FailureClassification::ContinuityFailed,
                format!(
                    "worktree branch {branch} does not match {}",
                    campaign.branch
                ),
            );
        }
        let head = self.git.current_head(&worktree)?;
        if head.value() != state.current_head_sha {
            return self.block(
                state,
                FailureClassification::ContinuityFailed,
                "worktree HEAD does not match recorded SHA",
            );
        }
        let snapshot = self.git.snapshot(&worktree)?;
        let dirty = source_paths(snapshot.changed_paths());
        if !dirty.is_empty() {
            if self.can_reset_interrupted_worktree(campaign, &state, &worktree, &dirty)? {
                self.persist_recovery_reset_evidence(&state, &snapshot, &dirty)?;
                self.git
                    .reset_managed_worktree(&worktree, &head, &dirty)
                    .map_err(|error| {
                        format!("failed to reconcile interrupted worktree: {error}")
                    })?;
            } else {
                return self.block(
                    state,
                    FailureClassification::ContinuityFailed,
                    format!(
                        "worktree has uncommitted source changes: {}",
                        dirty.join(", ")
                    ),
                );
            }
        }
        self.refresh_budget(campaign, &mut state);
        if state.remaining_seconds == 0 {
            state.state = CampaignState::Expired;
            state.blocked_reason =
                Some(FailureClassification::CampaignExpired.as_str().to_string());
            state.end_time = Some(self.now_text());
            self.save_state(&state)?;
            self.emit(
                &state.campaign_id,
                None,
                None,
                None,
                "campaign_expired",
                "campaign duration exhausted",
                Some(&state),
            )?;
            return Ok(state);
        }
        if let Err(error) = self.health.assert_executor() {
            return self.block(state, FailureClassification::ExecutorUnavailable, error);
        }
        let waited_for_workers = match self.recover_worker_health(campaign, &mut state)? {
            RecoveryHealthOutcome::Healthy { waited } => waited,
            RecoveryHealthOutcome::Stopped(stopped) => return Ok(stopped),
        };
        if waited_for_workers {
            state.current_action = Some("recovering".to_string());
            state.error_message = None;
            self.save_state(&state)?;
            self.emit(
                &state.campaign_id,
                None,
                None,
                None,
                "dependency_recovery_ready",
                "worker endpoint recovery completed within bound",
                Some(&state),
            )?;
        }
        self.emit(
            &state.campaign_id,
            None,
            None,
            None,
            "campaign_recovered",
            "recovery validation passed",
            Some(&state),
        )?;
        Ok(state)
    }

    fn recover_after_transient_model_request_failure(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
        step: &CampaignStep,
        worker_id: &str,
        recovery_attempts: &mut usize,
        recovery_started_at: u64,
        error: &str,
    ) -> Result<RecoveryHealthOutcome, String> {
        *recovery_attempts += 1;
        let elapsed = self.clock.now_unix().saturating_sub(recovery_started_at);
        if *recovery_attempts >= self.worker_recovery_max_attempts
            || elapsed >= self.worker_recovery_max_wait_seconds
        {
            let blocked = self.block(
                state.clone(),
                FailureClassification::ModelUnavailable,
                format!(
                    "worker endpoint {worker_id} remained inference-unready after bounded recovery wait (attempt {}/{}, elapsed {}s): {}",
                    *recovery_attempts,
                    self.worker_recovery_max_attempts,
                    elapsed,
                    error
                ),
            )?;
            *state = blocked.clone();
            return Ok(RecoveryHealthOutcome::Stopped(blocked));
        }
        state.current_action = Some("dependency_recovery_wait".to_string());
        state.error_message = Some(format!(
            "worker endpoint {worker_id} transient model-request startup failure after attempt {}/{}: {}",
            *recovery_attempts, self.worker_recovery_max_attempts, error
        ));
        self.save_state(state)?;
        self.emit(
            &state.campaign_id,
            Some(&step.id),
            Some(state.current_attempt),
            Some(worker_id),
            "dependency_recovery_waiting",
            format!(
                "worker endpoint {worker_id} transient model-request startup failure after attempt {}/{}: {}",
                *recovery_attempts,
                self.worker_recovery_max_attempts,
                error
            ),
            Some(state),
        )?;
        if self.recovery_checkpoint(campaign, state)?.is_some() {
            return Ok(RecoveryHealthOutcome::Stopped(state.clone()));
        }
        let delay = recovery_delay_seconds(
            &self.worker_recovery_retry_delays_seconds,
            *recovery_attempts,
        );
        self.sleeper.sleep_seconds(delay);
        if self.recovery_checkpoint(campaign, state)?.is_some() {
            return Ok(RecoveryHealthOutcome::Stopped(state.clone()));
        }
        state.current_action = Some("model_request".to_string());
        state.error_message = None;
        self.save_state(state)?;
        self.emit(
            &state.campaign_id,
            Some(&step.id),
            Some(state.current_attempt),
            Some(worker_id),
            "dependency_recovery_ready",
            "retrying model request after transient worker startup failure",
            Some(state),
        )?;
        Ok(RecoveryHealthOutcome::Healthy { waited: true })
    }

    fn recover_worker_health(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
    ) -> Result<RecoveryHealthOutcome, String> {
        let recovery_started_at = self.clock.now_unix();
        let mut attempts = 0usize;
        let mut waited = false;
        let worker_id = state
            .current_worker
            .as_deref()
            .unwrap_or(&campaign.worker_policy.primary)
            .to_string();
        loop {
            match self.health.assert_worker(&worker_id) {
                Ok(()) => return Ok(RecoveryHealthOutcome::Healthy { waited }),
                Err(error) => {
                    attempts += 1;
                    let elapsed = self.clock.now_unix().saturating_sub(recovery_started_at);
                    if attempts >= self.worker_recovery_max_attempts
                        || elapsed >= self.worker_recovery_max_wait_seconds
                    {
                        let blocked = self.block(
                            state.clone(),
                            FailureClassification::ModelUnavailable,
                            format!(
                                "worker endpoint {worker_id} unavailable after bounded recovery wait (attempt {attempts}/{}, elapsed {elapsed}s): {error}",
                                self.worker_recovery_max_attempts
                            ),
                        )?;
                        *state = blocked.clone();
                        return Ok(RecoveryHealthOutcome::Stopped(blocked));
                    }
                    waited = true;
                    state.current_action = Some("dependency_recovery_wait".to_string());
                    state.error_message = Some(format!(
                        "worker endpoint {worker_id} recovery pending after attempt {attempts}/{}: {error}",
                        self.worker_recovery_max_attempts
                    ));
                    self.save_state(state)?;
                    self.emit(
                        &state.campaign_id,
                        state.current_step_id.as_deref(),
                        Some(state.current_attempt),
                        Some(worker_id.as_str()),
                        "dependency_recovery_waiting",
                        format!(
                            "worker endpoint {worker_id} recovery pending after attempt {attempts}/{}: {error}",
                            self.worker_recovery_max_attempts
                        ),
                        Some(state),
                    )?;
                    if self.recovery_checkpoint(campaign, state)?.is_some() {
                        return Ok(RecoveryHealthOutcome::Stopped(state.clone()));
                    }
                    let delay = recovery_delay_seconds(
                        &self.worker_recovery_retry_delays_seconds,
                        attempts,
                    );
                    self.sleeper.sleep_seconds(delay);
                    if self.recovery_checkpoint(campaign, state)?.is_some() {
                        return Ok(RecoveryHealthOutcome::Stopped(state.clone()));
                    }
                }
            }
        }
    }

    fn recovery_checkpoint(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
    ) -> Result<Option<CampaignStatus>, String> {
        if let Ok(Some(disk)) = self.load_state(&state.campaign_id) {
            state.pause_requested = disk.pause_requested;
            state.cancel_requested = disk.cancel_requested;
        }
        self.refresh_budget(campaign, state);
        self.heartbeat(state)?;
        if state.cancel_requested {
            let finished = self.finish(
                state.clone(),
                FailureClassification::OperatorCancelled,
                "cancelled",
            )?;
            *state = finished.clone();
            return Ok(Some(finished));
        }
        if state.remaining_seconds == 0 {
            let finished = self.finish(
                state.clone(),
                FailureClassification::CampaignExpired,
                "campaign expired",
            )?;
            *state = finished.clone();
            return Ok(Some(finished));
        }
        if state.pause_requested {
            state.state = CampaignState::Paused;
            state.current_action = Some("paused".to_string());
            self.save_state(state)?;
            self.emit(
                &state.campaign_id,
                None,
                None,
                None,
                "campaign_paused",
                "paused during dependency recovery wait",
                Some(state),
            )?;
            return Ok(Some(state.clone()));
        }
        Ok(None)
    }

    fn can_reset_interrupted_worktree(
        &self,
        campaign: &Campaign,
        state: &CampaignStatus,
        worktree: &Path,
        dirty: &[String],
    ) -> Result<bool, String> {
        if state.state != CampaignState::Running {
            return Ok(false);
        }
        let action = state.current_action.as_deref().unwrap_or_default();
        if !matches!(
            action,
            "model_request" | "repair" | "fallback" | "git_inspect" | "acceptance_command"
        ) {
            return Ok(false);
        }
        let step_id = match state.current_step_id.as_deref() {
            Some(value) => value,
            None => return Ok(false),
        };
        let step_index = match find_step_index(&state.steps, step_id) {
            Ok(index) => index,
            Err(_) => return Ok(false),
        };
        let step_state = &state.steps[step_index];
        if step_state.accepted_commit.is_some()
            || state.current_attempt <= step_state.attempts.len()
        {
            return Ok(false);
        }
        let workspace_root = self.registry.workspace_root()?;
        if !worktree.starts_with(workspace_root.as_path()) {
            return Ok(false);
        }
        let step = campaign
            .steps
            .iter()
            .find(|candidate| candidate.id == step_id)
            .ok_or_else(|| format!("missing campaign step: {step_id}"))?;
        for path in dirty {
            if !path_is_authorized(path, &campaign.permitted_paths)? {
                return Ok(false);
            }
            if !path_is_authorized(path, &step.allowed_paths)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn persist_recovery_reset_evidence(
        &self,
        state: &CampaignStatus,
        snapshot: &GitEvidence,
        dirty: &[String],
    ) -> Result<(), String> {
        let action = state
            .current_action
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let step_id = state
            .current_step_id
            .clone()
            .unwrap_or_else(|| "unknown-step".to_string());
        let reason = format!(
            "reset interrupted campaign-owned dirty worktree before recovery resume during {action}"
        );
        write_json(
            self.campaign_dir(&state.campaign_id).join(format!(
                "recovery-reset-attempt-{}.json",
                state.current_attempt
            )),
            &RecoveryResetEvidenceDocument {
                step_id,
                attempt: state.current_attempt,
                worker_id: state.current_worker.clone(),
                action,
                reason,
                dirty_paths: dirty.to_vec(),
                diff_stat: snapshot.diff_stat().to_string(),
                diff_excerpt: bounded_excerpt(snapshot.diff(), 160),
                current_head_sha: state.current_head_sha.clone(),
                worktree_path: state.worktree_path.clone(),
            },
        )
    }

    fn preflight_live(&self, campaign: &Campaign, state: &CampaignStatus) -> Result<(), String> {
        self.validate_live_requirements(campaign)?;
        if !std::path::Path::new(&state.worktree_path).exists() {
            return Err("worktree is missing".to_string());
        }
        Ok(())
    }

    fn checkpoint(
        &self,
        campaign: &Campaign,
        state: &mut CampaignStatus,
        step: &CampaignStep,
        _attempt: Option<usize>,
    ) -> Result<Option<CampaignStatus>, String> {
        if let Ok(Some(disk)) = self.load_state(&state.campaign_id) {
            state.pause_requested = disk.pause_requested;
            state.cancel_requested = disk.cancel_requested;
        }
        self.refresh_budget(campaign, state);
        self.heartbeat(state)?;
        if state.cancel_requested {
            let finished = self.finish(
                state.clone(),
                FailureClassification::OperatorCancelled,
                "cancelled",
            )?;
            *state = finished.clone();
            return Ok(Some(finished));
        }
        if state.remaining_seconds == 0 {
            state.state = CampaignState::Expired;
            let finished = self.finish(
                state.clone(),
                FailureClassification::CampaignExpired,
                "campaign expired",
            )?;
            *state = finished.clone();
            return Ok(Some(finished));
        }
        if state.pause_requested {
            state.state = CampaignState::Paused;
            state.current_action = Some("paused".to_string());
            state.current_step_id = Some(step.id.clone());
            self.save_state(state)?;
            self.emit(
                &state.campaign_id,
                Some(&step.id),
                None,
                None,
                "campaign_paused",
                "paused at checkpoint",
                Some(state),
            )?;
            return Ok(Some(state.clone()));
        }
        Ok(None)
    }

    fn block(
        &self,
        mut state: CampaignStatus,
        classification: FailureClassification,
        reason: impl Into<String>,
    ) -> Result<CampaignStatus, String> {
        let reason = reason.into();
        state.state = match classification {
            FailureClassification::CampaignExpired => CampaignState::Expired,
            FailureClassification::OperatorCancelled => CampaignState::Cancelled,
            _ => CampaignState::Blocked,
        };
        state.blocked_reason = Some(classification.as_str().to_string());
        state.error_message = Some(reason.clone());
        state.end_time = Some(self.now_text());
        state.current_action = Some("blocked".to_string());
        self.save_state(&state)?;
        self.emit(
            &state.campaign_id,
            None,
            None,
            None,
            "campaign_blocked",
            reason,
            Some(&state),
        )?;
        Ok(state)
    }

    fn block_in_place(
        &self,
        state: &mut CampaignStatus,
        classification: FailureClassification,
        reason: impl Into<String>,
    ) -> Result<(), String> {
        *state = self.block(state.clone(), classification, reason)?;
        Ok(())
    }

    fn finish(
        &self,
        mut state: CampaignStatus,
        classification: FailureClassification,
        message: &str,
    ) -> Result<CampaignStatus, String> {
        state.state = match classification {
            FailureClassification::CampaignExpired => CampaignState::Expired,
            FailureClassification::OperatorCancelled => CampaignState::Cancelled,
            _ => state.state,
        };
        state.blocked_reason = Some(classification.as_str().to_string());
        state.end_time = Some(self.now_text());
        self.save_state(&state)?;
        let event_type = match classification {
            FailureClassification::CampaignExpired => "campaign_expired",
            FailureClassification::OperatorCancelled => "campaign_cancelled",
            _ => "campaign_blocked",
        };
        self.emit(
            &state.campaign_id,
            None,
            None,
            None,
            event_type,
            message,
            Some(&state),
        )?;
        Ok(state)
    }

    fn heartbeat(&self, state: &mut CampaignStatus) -> Result<(), String> {
        let now = self.now_text();
        state.last_heartbeat = now.clone();
        self.refresh_budget_with(state, now.parse::<u64>().unwrap_or(0));
        self.leases
            .heartbeat(&state.campaign_id, &state.repository_id, &now)?;
        self.save_state(state)?;
        self.emit(
            &state.campaign_id,
            state.current_step_id.as_deref(),
            None,
            state.current_worker.as_deref(),
            "heartbeat",
            "runner heartbeat",
            Some(state),
        )?;
        Ok(())
    }

    fn refresh_budget(&self, campaign: &Campaign, state: &mut CampaignStatus) {
        let now = self.clock.now_unix();
        self.refresh_budget_with(state, now);
        let start = state.start_time.parse::<u64>().unwrap_or(now);
        let elapsed = now.saturating_sub(start);
        state.duration_seconds = elapsed;
        state.remaining_seconds = campaign.limits.max_runtime_seconds.saturating_sub(elapsed);
    }

    fn refresh_budget_with(&self, state: &mut CampaignStatus, now: u64) {
        let start = state.start_time.parse::<u64>().unwrap_or(now);
        state.duration_seconds = now.saturating_sub(start);
    }

    fn require_state(&self, campaign_id: &str) -> Result<CampaignStatus, String> {
        self.load_state(campaign_id)?
            .ok_or_else(|| format!("campaign state not found: {campaign_id}"))
    }

    fn now_text(&self) -> String {
        self.clock.now_unix().to_string()
    }

    fn emit(
        &self,
        campaign_id: &str,
        step_id: Option<&str>,
        attempt: Option<usize>,
        worker_id: Option<&str>,
        event_type: &str,
        message: impl Into<String>,
        state: Option<&CampaignStatus>,
    ) -> Result<(), String> {
        self.log_event(CampaignEvent {
            timestamp: self.now_text(),
            campaign_id: campaign_id.to_string(),
            step_id: step_id.map(|value| value.to_string()),
            attempt,
            worker_id: worker_id.map(|value| value.to_string()),
            action: state.and_then(|item| item.current_action.clone()),
            state: state.map(|item| format!("{:?}", item.state).to_lowercase()),
            event_type: event_type.to_string(),
            message: message.into(),
            details: serde_json::Map::new(),
        })
    }
}

enum StepOutcome {
    Accepted,
    Stopped,
}

enum RecoveryHealthOutcome {
    Healthy { waited: bool },
    Stopped(CampaignStatus),
}

fn update_background_state_heartbeat(
    state_root: &PathBuf,
    campaign_id: &str,
    step_id: Option<&str>,
    worker_id: Option<&str>,
    action: &str,
) -> Result<(), String> {
    let campaign_dir = state_root.join("state").join("campaigns").join(campaign_id);
    let state_path = campaign_dir.join("state.json");
    let _lock = CampaignLock::acquire(&campaign_dir)?;
    let campaign_path = campaign_dir.join("campaign.json");
    let mut state = crate::load_campaign_status_compatible(&state_path, Some(&campaign_path))?
        .ok_or_else(|| format!("campaign state not found: {campaign_id}"))?;
    if !matches!(state.state, CampaignState::Running) {
        return Err(format!("campaign {campaign_id} is no longer running"));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    state.last_heartbeat = now;
    state.current_action = Some(action.to_string());
    if let Some(step_id) = step_id {
        state.current_step_id = Some(step_id.to_string());
    }
    if let Some(worker_id) = worker_id {
        state.current_worker = Some(worker_id.to_string());
    }
    let json = serde_json::to_string_pretty(&state).map_err(|error| error.to_string())?;
    atomic_write(&state_path, &format!("{json}\n"))
}

fn stopped_from(_state: CampaignStatus) -> StepOutcome {
    StepOutcome::Stopped
}

trait FromPreflight {
    fn from_preflight(error: &str) -> Self;
}

impl FromPreflight for FailureClassification {
    fn from_preflight(error: &str) -> Self {
        if error.contains("podman") || error.contains("executor") {
            FailureClassification::ExecutorUnavailable
        } else if error.contains("worker") || error.contains("endpoint") {
            FailureClassification::ModelUnavailable
        } else {
            FailureClassification::ContinuityFailed
        }
    }
}

fn pending_step(step: &CampaignStep) -> StepStatusRecord {
    StepStatusRecord {
        step_id: step.id.clone(),
        kind: match step.kind {
            CampaignStepKind::Implementation => "implementation".to_string(),
            CampaignStepKind::Verification => "verification".to_string(),
        },
        disposition: "pending".to_string(),
        review_disposition: None,
        review_rationale: None,
        attempts: Vec::new(),
        accepted_commit: None,
    }
}

fn find_step_index(steps: &[StepStatusRecord], step_id: &str) -> Result<usize, String> {
    steps
        .iter()
        .position(|step| step.step_id == step_id)
        .ok_or_else(|| format!("missing step status: {step_id}"))
}

fn total_attempts(state: &CampaignStatus) -> usize {
    state.steps.iter().map(|step| step.attempts.len()).sum()
}

fn attempt_kind_name(kind: AttemptKind) -> &'static str {
    match kind {
        AttemptKind::Primary => "primary",
        AttemptKind::Repair => "repair",
        AttemptKind::Fallback => "fallback",
        AttemptKind::Verification => "verification",
    }
}

fn recovery_decision_name(kind: RecoveryDecisionKind) -> &'static str {
    match kind {
        RecoveryDecisionKind::Repair => "repair",
        RecoveryDecisionKind::Replan => "replan",
        RecoveryDecisionKind::BlockInsufficientAuthority => "block_insufficient_authority",
        RecoveryDecisionKind::BlockTerminal => "block_terminal",
        RecoveryDecisionKind::RetryTransient => "retry_transient",
    }
}

fn should_diagnose_retryable_failure(
    classification: FailureClassification,
    repeated_failure_count: usize,
) -> bool {
    matches!(
        classification,
        FailureClassification::AcceptanceFailed
            | FailureClassification::ArtifactMissing
            | FailureClassification::InadequateImplementation
    ) || (classification == FailureClassification::NoChange && repeated_failure_count > 0)
        || (classification == FailureClassification::ToolProtocolViolation
            && repeated_failure_count > 0)
}

fn command_failure(commands: &[CommandEvidence]) -> Option<RecoveryCommandFailure> {
    let failed = commands.iter().find(|command| !command.succeeded())?;
    Some(RecoveryCommandFailure {
        command: failed.argv().join(" "),
        exit_code: failed.exit_code(),
        stdout_excerpt: bounded_chars(failed.stdout(), 600),
        stderr_excerpt: bounded_chars(failed.stderr(), 600),
    })
}

fn tool_attempts(step: &CampaignStep, result: &ImplementChangeResult) -> Vec<RecoveryToolAttempt> {
    result
        .tool_calls()
        .iter()
        .map(|call| {
            let target_path = extract_target_path(&call.arguments);
            let allowed = target_path
                .as_deref()
                .and_then(|path| path_is_authorized(path, &step.allowed_paths).ok());
            RecoveryToolAttempt {
                name: call.name.clone(),
                target_path,
                allowed,
                result_excerpt: bounded_chars(&call.result, 240),
            }
        })
        .collect()
}

fn tool_attempts_for_fingerprint(result: &ImplementChangeResult) -> Vec<String> {
    result
        .tool_calls()
        .iter()
        .filter_map(|call| {
            let path = extract_target_path(&call.arguments)?;
            Some(format!("{}:{}", call.name, path))
        })
        .collect()
}

fn extract_target_path(arguments: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(arguments).ok()?;
    value
        .get("file_path")
        .or_else(|| value.get("path"))
        .or_else(|| value.get("target_path"))
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

fn bounded_chars(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut output = trimmed.chars().take(max_chars).collect::<String>();
    if trimmed.chars().count() > max_chars {
        output.push_str("
[truncated]");
    }
    output
}

fn first_non_empty_line(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no-detail")
        .to_string()
}

fn allowed_paths(values: &[String]) -> Result<AllowedPaths, String> {
    AllowedPaths::new(
        values
            .iter()
            .cloned()
            .map(AllowedPath::new)
            .collect::<Result<Vec<_>, _>>()?,
    )
}

fn looks_protocol(result: &ImplementChangeResult) -> bool {
    result.protocol_error().is_some() || crate::looks_like_markdown_tool_call(result.output())
}

fn is_executor_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("podman") || lower.contains("executor")
}

fn implementer_error_result(error: String) -> ImplementChangeResult {
    let lower = error.to_lowercase();
    if lower.contains("tool") || lower.contains("finish_reason") || lower.contains("markdown") {
        ImplementChangeResult::new(String::new()).with_protocol_error(error)
    } else {
        ImplementChangeResult::new(String::new()).with_worker_error(error)
    }
}

fn is_transient_worker_transport_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("connection refused")
        || lower.contains("peer disconnected")
        || lower.contains("connection reset")
        || lower.contains("reset by peer")
        || lower.contains("peer reset")
        || lower.contains("temporary http transport failure")
}

fn empty_evidence(sha: &str) -> Result<GitEvidence, String> {
    Ok(GitEvidence::new(
        GitSha::new(sha.to_string())?,
        String::new(),
    ))
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, format!("{json}\n")).map_err(|error| error.to_string())
}

fn recovery_delay_seconds(delays: &[u64], attempt: usize) -> u64 {
    if delays.is_empty() {
        return 1;
    }
    let index = attempt
        .saturating_sub(1)
        .min(delays.len().saturating_sub(1));
    delays[index]
}

fn bounded_excerpt(value: &str, max_lines: usize) -> String {
    let mut lines = value.lines();
    let excerpt = lines
        .by_ref()
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n");
    if lines.next().is_some() {
        format!("{excerpt}\n... truncated ...")
    } else {
        excerpt
    }
}
