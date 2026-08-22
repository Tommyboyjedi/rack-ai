use std::fs;
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
use crate::assert_step_paths_permitted;
use crate::atomic_write;
use crate::campaign_digest;
use crate::durable_file::append_line;
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
    state_root: PathBuf,
    leases: CampaignLeaseStore,
    container_tracker: Option<Arc<CampaignContainerTracker>>,
    reviewer: Option<&'a dyn ImplementationReviewer>,
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
    review: CoordinatorReview,
    changed_paths: Vec<String>,
    commit_sha: Option<String>,
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
            state_root: dependencies.state_root,
            leases,
            container_tracker: dependencies.container_tracker,
            reviewer: None,
        }
    }

    pub fn with_reviewer(mut self, reviewer: &'a dyn ImplementationReviewer) -> Self {
        self.reviewer = Some(reviewer);
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
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        let state =
            serde_json::from_str::<CampaignStatus>(&content).map_err(|error| error.to_string())?;
        Ok(Some(state))
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
        self.validate_document(campaign)?;
        self.health.assert_workers(
            &campaign.worker_policy.primary,
            &campaign.worker_policy.fallback,
        )?;
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
        let mut last_attempt = 0usize;
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
            let (kind, worker_id, repair_of, fallback_of) = if primary_left > 0 {
                primary_left -= 1;
                (
                    AttemptKind::Primary,
                    campaign.worker_policy.primary.clone(),
                    None,
                    None,
                )
            } else if repair_left > 0 {
                repair_left -= 1;
                (
                    AttemptKind::Repair,
                    campaign.worker_policy.primary.clone(),
                    Some(last_attempt),
                    None,
                )
            } else if fallback_left > 0 {
                fallback_left -= 1;
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
                _ => match &last_review {
                    Some(review) => repair_instruction(step, review, &last_evidence_summary),
                    None => step.task.clone(),
                },
            };
            if kind != AttemptKind::Primary {
                self.emit(
                    &state.campaign_id,
                    Some(&step.id),
                    Some(attempt_number),
                    Some(runtime.worker_id.as_str()),
                    "repair_instruction_recorded",
                    "bounded repair instruction persisted",
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
            if let Some(stopped) = self.checkpoint(campaign, state, step, None)? {
                self.persist_partial_failure(
                    state,
                    step,
                    attempt_number,
                    kind,
                    &runtime.worker_id,
                    &start,
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
                        self.action_timeout_seconds(state, step),
                    );
                    self.add_untracked_review_evidence(state, &mut model_request);
                    let model_review = reviewer.review(&model_request);
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
                                "disposition": CoordinatorReviewDisposition::RejectedRetryable,
                                "classification": FailureClassification::ModelUnavailable,
                                "rationale": format!("model reviewer failed closed: {error}"),
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
                            review.disposition = CoordinatorReviewDisposition::RejectedRetryable;
                            review.classification = Some(FailureClassification::ModelUnavailable);
                            review.rationale = format!("model reviewer failed closed: {error}");
                        }
                    }
                    review.evidence_refs.push("model-review.json".to_string());
                }
            }
            if review.disposition == CoordinatorReviewDisposition::RejectedRetryable {
                review.repair_instruction =
                    Some(repair_instruction(step, &review, evidence.diff_stat()));
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
                    review = pre_commit_review;
                    last_review = Some(review.clone());
                } else {
                    if let Some(stopped) =
                        self.checkpoint(campaign, state, step, Some(attempt_number))?
                    {
                        self.persist_attempt(
                            state,
                            step,
                            attempt_number,
                            kind,
                            runtime.worker_id.as_str(),
                            &start,
                            repair_of,
                            fallback_of,
                            Some(&implement_result),
                            &commands,
                            &pre_commit,
                            &review,
                            None,
                        )?;
                        return Ok(stopped_from(stopped));
                    }

                    let sha = self.git.commit_local(&CampaignCommitRequest::new(
                        PathBuf::from(&state.worktree_path),
                        &campaign.campaign_id,
                        &step.id,
                        source_paths(pre_commit.changed_paths()),
                    ))?;
                    commit_sha = Some(sha.value().to_string());
                    state.current_head_sha = sha.value().to_string();
                    self.emit(
                        &state.campaign_id,
                        Some(&step.id),
                        Some(attempt_number),
                        Some(runtime.worker_id.as_str()),
                        "git_commit",
                        format!("created local commit {}", sha.value()),
                        Some(state),
                    )?;
                }
            }
            self.persist_attempt(
                state,
                step,
                attempt_number,
                kind,
                runtime.worker_id.as_str(),
                &start,
                repair_of,
                fallback_of,
                Some(&implement_result),
                &commands,
                &evidence,
                &review,
                commit_sha.clone(),
            )?;
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
                }
            }
        }
    }

    fn persist_partial_failure(
        &self,
        state: &mut CampaignStatus,
        step: &CampaignStep,
        attempt: usize,
        kind: AttemptKind,
        worker_id: &str,
        start: &str,
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
        let packet = ReviewPacketDocument {
            step_id: step.id.clone(),
            attempt,
            worker_id: worker_id.to_string(),
            attempt_kind: kind,
            review: review.clone(),
            changed_paths: source_paths(evidence.changed_paths()),
            commit_sha: commit_sha.clone(),
        };
        write_json(dir.join("review-packet.json"), &packet)?;
        let transcript = serde_json::json!({
            "worker_id": worker_id,
            "attempt_kind": kind,
            "output": implement_result.map(|item| item.output().to_string()),
            "protocol_error": implement_result.and_then(|item| item.protocol_error().map(|value| value.to_string())),
            "executor_kind": implement_result.map(|item| item.executor_kind().to_string()),
            "used_host_shell": implement_result.map(|item| item.used_host_shell()).unwrap_or(false),
            "tool_calls": implement_result.map(|item| item.tool_calls().iter().map(|call| serde_json::json!({
                "name": call.name,
                "arguments": call.arguments,
                "result": call.result,
            })).collect::<Vec<_>>()).unwrap_or_default(),
            "repair_instruction": review.repair_instruction,
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
            repair_instruction: review.repair_instruction.clone(),
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
            return self.block(
                state,
                FailureClassification::ContinuityFailed,
                format!(
                    "worktree has uncommitted source changes: {}",
                    dirty.join(", ")
                ),
            );
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
        if let Err(error) = self.health.assert_workers(
            &campaign.worker_policy.primary,
            &campaign.worker_policy.fallback,
        ) {
            return self.block(state, FailureClassification::ModelUnavailable, error);
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

    fn preflight_live(&self, campaign: &Campaign, state: &CampaignStatus) -> Result<(), String> {
        self.validate(campaign)?;
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
    let content = fs::read_to_string(&state_path).map_err(|error| error.to_string())?;
    let mut state =
        serde_json::from_str::<CampaignStatus>(&content).map_err(|error| error.to_string())?;
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
