use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::CampaignCleanupAction;
use crate::CampaignContainerTracker;
use crate::CampaignLeaseRecord;
use crate::CampaignLeaseStore;
use crate::CampaignRunner;
use crate::CampaignState;
use crate::CampaignStatus;
use crate::CampaignSupervisionAction;
use crate::CampaignSupervisionReport;
use crate::FailureClassification;
use crate::OperationsConfig;
use crate::UnixClock;
use crate::campaign_lease::lease_is_stale;

pub struct CampaignSupervisor<'a> {
    runner: &'a CampaignRunner<'a>,
    clock: &'a dyn UnixClock,
    state_root: PathBuf,
    workspace_root: PathBuf,
    operations: OperationsConfig,
}

pub struct CampaignSupervisorDependencies<'a> {
    pub runner: &'a CampaignRunner<'a>,
    pub clock: &'a dyn UnixClock,
    pub state_root: PathBuf,
    pub workspace_root: PathBuf,
    pub operations: OperationsConfig,
}

impl<'a> CampaignSupervisor<'a> {
    pub fn new(dependencies: CampaignSupervisorDependencies<'a>) -> Result<Self, String> {
        dependencies.operations.validate()?;
        Ok(Self {
            runner: dependencies.runner,
            clock: dependencies.clock,
            state_root: dependencies.state_root,
            workspace_root: dependencies.workspace_root,
            operations: dependencies.operations,
        })
    }

    pub fn run_once(&self) -> Result<CampaignSupervisionReport, String> {
        let states = self.load_states()?;
        let mut report = CampaignSupervisionReport::new();
        report.scanned_campaigns = states.len();
        for state in &states {
            let previous = state.state;
            if previous == CampaignState::Running {
                if !self.cleanup_stale_campaign_container(state, &mut report)? {
                    continue;
                }
                if self.operations.supervisor.resume_running_campaigns {
                    match self.runner.resume(&state.campaign_id) {
                        Ok(updated) => {
                            report.resumed_campaigns += 1;
                            report.actions.push(CampaignSupervisionAction {
                                campaign_id: state.campaign_id.clone(),
                                previous_state: previous,
                                action: "resume".to_string(),
                                outcome_state: Some(updated.state),
                                message: format!("resume completed with {:?}", updated.state),
                            });
                        }
                        Err(error) => {
                            report.actions.push(CampaignSupervisionAction {
                                campaign_id: state.campaign_id.clone(),
                                previous_state: previous,
                                action: "resume_failed".to_string(),
                                outcome_state: None,
                                message: error,
                            });
                        }
                    }
                    continue;
                }
            }
            report.actions.push(CampaignSupervisionAction {
                campaign_id: state.campaign_id.clone(),
                previous_state: previous,
                action: "observe".to_string(),
                outcome_state: Some(previous),
                message: format!("campaign left in {:?}", previous),
            });
        }
        report
            .cleanup
            .extend(self.prune_terminal_campaigns(&states)?);
        report.cleanup.extend(self.prune_auxiliary_artifacts()?);
        report
            .cleanup
            .extend(self.cleanup_orphan_repository_leases()?);
        Ok(report)
    }

    fn load_states(&self) -> Result<Vec<CampaignStatus>, String> {
        let mut states = Vec::new();
        let dir = self.state_root.join("state").join("campaigns");
        if !dir.exists() {
            return Ok(states);
        }
        for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            if let Some(state) = self.runner.load_state(name)? {
                states.push(state);
            }
        }
        states.sort_by(|left, right| left.campaign_id.cmp(&right.campaign_id));
        Ok(states)
    }

    fn cleanup_stale_campaign_container(
        &self,
        state: &CampaignStatus,
        report: &mut CampaignSupervisionReport,
    ) -> Result<bool, String> {
        let active_path = self
            .runner
            .campaign_dir(&state.campaign_id)
            .join("active-container.json");
        if state.active_container_id.is_none() && !active_path.exists() {
            return Ok(true);
        }
        let tracker = CampaignContainerTracker::new(self.state_root.clone());
        match tracker.cleanup_campaign_container(
            &state.campaign_id,
            &self.operations.supervisor.podman_command,
        ) {
            Ok(Some(container_id)) => {
                report.cleanup.push(CampaignCleanupAction {
                    campaign_id: state.campaign_id.clone(),
                    action: "cleanup_stale_campaign_container".to_string(),
                    message: format!("removed stale campaign container {container_id}"),
                });
                Ok(true)
            }
            Ok(None) => Ok(true),
            Err(error) => {
                let blocked = self.runner.mark_supervisor_blocked(
                    &state.campaign_id,
                    FailureClassification::ExecutorUnavailable,
                    format!("supervisor container cleanup failed: {error}"),
                )?;
                report.actions.push(CampaignSupervisionAction {
                    campaign_id: state.campaign_id.clone(),
                    previous_state: state.state,
                    action: "container_cleanup_failed".to_string(),
                    outcome_state: Some(blocked.state),
                    message: error,
                });
                Ok(false)
            }
        }
    }

    fn prune_terminal_campaigns(
        &self,
        states: &[CampaignStatus],
    ) -> Result<Vec<CampaignCleanupAction>, String> {
        let mut terminal = states
            .iter()
            .filter(|state| is_terminal(state.state))
            .cloned()
            .collect::<Vec<_>>();
        terminal.sort_by(|left, right| terminal_timestamp(right).cmp(&terminal_timestamp(left)));
        let mut actions = Vec::new();
        let now = self.clock.now_unix();
        for (index, state) in terminal.iter().enumerate() {
            if index < self.operations.retention.retain_terminal_campaigns {
                continue;
            }
            let ended_at = terminal_timestamp(state);
            if now.saturating_sub(ended_at)
                < self.operations.retention.max_terminal_campaign_age_seconds
            {
                continue;
            }
            actions.push(self.prune_campaign(state)?);
        }
        Ok(actions)
    }

    fn prune_campaign(&self, state: &CampaignStatus) -> Result<CampaignCleanupAction, String> {
        let lease_store = CampaignLeaseStore::new(self.state_root.clone());
        let _ = lease_store.release(&state.campaign_id, &state.repository_id);
        let worktree = PathBuf::from(&state.worktree_path);
        let mut removed = Vec::new();
        if worktree.exists() && worktree.starts_with(&self.workspace_root) {
            fs::remove_dir_all(&worktree).map_err(|error| error.to_string())?;
            removed.push(format!("worktree {}", worktree.display()));
        }
        let campaign_dir = self.runner.campaign_dir(&state.campaign_id);
        if campaign_dir.exists() {
            fs::remove_dir_all(&campaign_dir).map_err(|error| error.to_string())?;
            removed.push(format!("campaign state {}", campaign_dir.display()));
        }
        Ok(CampaignCleanupAction {
            campaign_id: state.campaign_id.clone(),
            action: "prune_terminal_campaign".to_string(),
            message: removed.join(", "),
        })
    }

    fn prune_auxiliary_artifacts(&self) -> Result<Vec<CampaignCleanupAction>, String> {
        let mut actions = Vec::new();
        let retain = self.operations.retention.retain_auxiliary_artifacts;
        let age = self.operations.retention.max_auxiliary_artifact_age_seconds;
        let targets = [
            self.state_root.join("logs").join("runs"),
            self.state_root.join("logs").join("specs"),
            self.state_root.join("state").join("runs"),
            self.state_root.join("state").join("queue").join("history"),
            self.state_root.join("state").join("changes"),
        ];
        for dir in targets {
            actions.extend(self.prune_directory_entries(&dir, age, retain)?);
        }
        Ok(actions)
    }

    fn prune_directory_entries(
        &self,
        dir: &PathBuf,
        max_age_seconds: u64,
        retain_count: usize,
    ) -> Result<Vec<CampaignCleanupAction>, String> {
        let mut actions = Vec::new();
        if !dir.exists() {
            return Ok(actions);
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let modified = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push((path, modified));
        }
        entries.sort_by(|left, right| right.1.cmp(&left.1));
        let now = self.clock.now_unix();
        for (index, (path, modified)) in entries.into_iter().enumerate() {
            if index < retain_count {
                continue;
            }
            let modified_unix = modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now.saturating_sub(modified_unix) < max_age_seconds {
                continue;
            }
            if path.is_dir() {
                fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
            } else {
                fs::remove_file(&path).map_err(|error| error.to_string())?;
            }
            actions.push(CampaignCleanupAction {
                campaign_id: path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("auxiliary")
                    .to_string(),
                action: "prune_auxiliary_artifact".to_string(),
                message: format!("removed {}", path.display()),
            });
        }
        Ok(actions)
    }

    fn cleanup_orphan_repository_leases(&self) -> Result<Vec<CampaignCleanupAction>, String> {
        let mut actions = Vec::new();
        let dir = self
            .state_root
            .join("state")
            .join("campaigns")
            .join(".repository-leases");
        if !dir.exists() {
            return Ok(actions);
        }
        let now = self.clock.now_unix().to_string();
        for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let record = serde_json::from_str::<CampaignLeaseRecord>(&content)
                .map_err(|error| error.to_string())?;
            let campaign_dir = self.runner.campaign_dir(&record.campaign_id);
            if campaign_dir.exists() {
                continue;
            }
            if !lease_is_stale(
                &record,
                &now,
                record.heartbeat_seconds,
                record.action_timeout_seconds,
            ) {
                continue;
            }
            fs::remove_file(&path).map_err(|error| error.to_string())?;
            actions.push(CampaignCleanupAction {
                campaign_id: record.campaign_id,
                action: "remove_orphan_repository_lease".to_string(),
                message: format!("removed stale lease {}", path.display()),
            });
        }
        Ok(actions)
    }
}

fn is_terminal(state: CampaignState) -> bool {
    matches!(
        state,
        CampaignState::Completed
            | CampaignState::Cancelled
            | CampaignState::Expired
            | CampaignState::Failed
    )
}

fn terminal_timestamp(state: &CampaignStatus) -> u64 {
    state
        .end_time
        .as_deref()
        .unwrap_or(state.last_heartbeat.as_str())
        .parse::<u64>()
        .unwrap_or(0)
}
