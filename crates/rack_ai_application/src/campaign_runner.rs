use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rack_ai_domain::AcceptanceCommand;
use rack_ai_domain::GitRef;
use rack_ai_domain::RepositoryId;

use crate::Campaign;
use crate::CampaignEvent;
use crate::CampaignState;
use crate::CampaignStatus;
use crate::CommandPolicy;
use crate::CreateChangeWorktreeRequest;
use crate::GitWorktree;
use crate::RepositoryRegistry;
use crate::ResolveGitShaRequest;
use crate::StepStatusRecord;

pub struct CampaignRunner<'a> {
    registry: &'a dyn RepositoryRegistry,
    command_policy: &'a dyn CommandPolicy,
    git: &'a dyn GitWorktree,
    state_root: PathBuf,
}

impl<'a> CampaignRunner<'a> {
    pub fn new(
        registry: &'a dyn RepositoryRegistry,
        command_policy: &'a dyn CommandPolicy,
        git: &'a dyn GitWorktree,
        state_root: PathBuf,
    ) -> Self {
        Self {
            registry,
            command_policy,
            git,
            state_root,
        }
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
        let state = serde_json::from_str::<CampaignStatus>(&content)
            .map_err(|error| error.to_string())?;
        Ok(Some(state))
    }

    pub fn save_state(&self, state: &CampaignStatus) -> Result<(), String> {
        let dir = self.campaign_dir(&state.campaign_id);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let json = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
        fs::write(self.state_path(&state.campaign_id), format!("{json}\n"))
            .map_err(|error| error.to_string())
    }

    pub fn log_event(&self, event: CampaignEvent) -> Result<(), String> {
        let path = self.events_path(&event.campaign_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        let line = serde_json::to_string(&event).map_err(|error| error.to_string())?;
        writeln!(file, "{line}").map_err(|error| error.to_string())
    }

    pub fn validate(&self, campaign: &Campaign) -> Result<(), String> {
        if campaign.version != "rack-ai/campaign/v1" {
            return Err("unsupported campaign version".to_string());
        }
        if campaign.campaign_id.trim().is_empty() {
            return Err("campaign_id cannot be empty".to_string());
        }
        let expected_branch = format!("rack/campaign-{}", campaign.campaign_id);
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
                crate::CampaignStepKind::Implementation => {
                    if step.required_changed_paths.is_empty() {
                        return Err(format!(
                            "step {} required_changed_paths cannot be empty for implementation steps",
                            step.id
                        ));
                    }
                }
                crate::CampaignStepKind::Verification => {
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
        let base_sha = self.git.resolve_sha(&ResolveGitShaRequest::new(
            repository.root().to_path_buf(),
            GitRef::new(campaign.repository.base_ref.clone())?,
        ))?;
        let workspace_root = self.registry.workspace_root()?;
        let worktree_path = workspace_root
            .join(format!("campaign-{}", campaign.campaign_id).as_str())
            .join("repo");
        let _workspace = self.git.create(
            &CreateChangeWorktreeRequest::new(repository.root().to_path_buf(), base_sha.clone())
                .with_branch_name(campaign.branch.clone())
                .with_worktree_path(worktree_path.clone()),
        )?;
        let dir = self.campaign_dir(&campaign.campaign_id);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let campaign_json =
            serde_json::to_string_pretty(campaign).map_err(|error| error.to_string())?;
        fs::write(dir.join("campaign.json"), format!("{campaign_json}\n"))
            .map_err(|error| error.to_string())?;
        let now = now_text();
        let state = CampaignStatus {
            schema_version: "rack-ai/campaign/v1".to_string(),
            campaign_id: campaign.campaign_id.clone(),
            campaign_digest: digest_for(campaign)?,
            repository_id: campaign.repository.id.clone(),
            base_sha: campaign.repository.base_sha.clone(),
            branch: campaign.branch.clone(),
            worktree_path: worktree_path.to_string_lossy().to_string(),
            current_head_sha: base_sha.value().to_string(),
            state: CampaignState::Running,
            current_step_id: None,
            current_attempt: 0,
            pause_requested: false,
            cancel_requested: false,
            start_time: now.clone(),
            end_time: None,
            duration_seconds: 0,
            remaining_seconds: campaign.limits.max_runtime_seconds,
            last_heartbeat: now.clone(),
            steps: campaign
                .steps
                .iter()
                .map(|step| StepStatusRecord {
                    step_id: step.id.clone(),
                    kind: serde_json::to_value(&step.kind)
                        .ok()
                        .and_then(|value| value.as_str().map(|text| text.to_string()))
                        .unwrap_or_else(|| "implementation".to_string()),
                    disposition: "pending".to_string(),
                    attempts: Vec::new(),
                    accepted_commit: None,
                })
                .collect(),
            active_container_id: None,
            error_message: None,
            blocked_reason: None,
        };
        self.save_state(&state)?;
        self.log_event(CampaignEvent {
            timestamp: now,
            campaign_id: campaign.campaign_id.clone(),
            step_id: None,
            attempt: None,
            event_type: "campaign_started".to_string(),
            message: "campaign state initialized".to_string(),
            details: serde_json::Map::new(),
        })?;
        Ok(state)
    }
}

fn digest_for(campaign: &Campaign) -> Result<String, String> {
    let json = serde_json::to_string(campaign).map_err(|error| error.to_string())?;
    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn now_text() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_domain::AcceptanceCommand;
    use rack_ai_domain::ChangeId;
    use rack_ai_domain::GitSha;
    use rack_ai_domain::RepositoryId;

    use super::CampaignRunner;
    use crate::Campaign;
    use crate::CampaignLimits;
    use crate::CampaignRepository;
    use crate::CampaignState;
    use crate::CampaignStep;
    use crate::CampaignStepKind;
    use crate::ChangeWorkspace;
    use crate::CommandPolicy;
    use crate::CreateChangeWorktreeRequest;
    use crate::ExecutorConfig;
    use crate::GitEvidence;
    use crate::GitWorktree;
    use crate::InspectChangeWorktreeRequest;
    use crate::RegisteredRepository;
    use crate::RepositoryRegistry;
    use crate::ResolveGitShaRequest;
    use crate::StepAcceptance;
    use crate::StepLimits;
    use crate::WorkerPolicy;
    use crate::WorkspaceRoot;

    #[test]
    fn validate_rejects_empty_acceptance_commands() {
        let root = temp_root();
        let registry = FakeRegistry::new(root.join("repo"));
        let policy = AllowAllPolicy;
        let git = FakeGit::new("a".repeat(40));
        let runner = CampaignRunner::new(&registry, &policy, &git, root.clone());
        let mut campaign = sample_campaign("a".repeat(40));
        campaign.steps[0].acceptance.commands.clear();

        let error = runner.validate(&campaign).unwrap_err();

        assert_eq!(
            error,
            "step step-1 acceptance.commands cannot be empty".to_string()
        );
    }

    #[test]
    fn start_persists_campaign_state_and_document() {
        let root = temp_root();
        let registry = FakeRegistry::new(root.join("repo"));
        let policy = AllowAllPolicy;
        let git = FakeGit::new("b".repeat(40));
        let runner = CampaignRunner::new(&registry, &policy, &git, root.clone());
        let campaign = sample_campaign("b".repeat(40));

        let state = runner.start(&campaign).unwrap();

        assert_eq!(state.state, CampaignState::Running);
        assert_eq!(state.branch, "rack/campaign-campaign-1");
        assert!(runner.state_path("campaign-1").exists());
        assert!(runner.campaign_dir("campaign-1").join("campaign.json").exists());
        let event_log = fs::read_to_string(runner.events_path("campaign-1")).unwrap();
        assert!(event_log.contains("campaign_started"));
    }

    struct FakeRegistry {
        repository: RegisteredRepository,
        workspace_root: WorkspaceRoot,
        executor_config: ExecutorConfig,
    }

    impl FakeRegistry {
        fn new(repository_root: PathBuf) -> Self {
            fs::create_dir_all(&repository_root).unwrap();
            let repository = RegisteredRepository::new(
                RepositoryId::new("adaptos".to_string()).unwrap(),
                repository_root,
            )
            .unwrap();
            Self {
                repository,
                workspace_root: WorkspaceRoot::new(PathBuf::from("/tmp/rack-workspaces")).unwrap(),
                executor_config: ExecutorConfig::podman(
                    "docker.io/library/rust:bookworm".to_string(),
                )
                .unwrap(),
            }
        }
    }

    impl RepositoryRegistry for FakeRegistry {
        fn workspace_root(&self) -> Result<WorkspaceRoot, String> {
            Ok(self.workspace_root.clone())
        }

        fn executor_config(&self) -> Result<ExecutorConfig, String> {
            Ok(self.executor_config.clone())
        }

        fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
            if self.repository.id() != id {
                return Err(format!("repository {} is not registered", id.value()));
            }
            Ok(self.repository.clone())
        }
    }

    struct AllowAllPolicy;

    impl CommandPolicy for AllowAllPolicy {
        fn assert_allowed(&self, _command: &AcceptanceCommand) -> Result<(), String> {
            Ok(())
        }
    }

    struct FakeGit {
        sha: String,
    }

    impl FakeGit {
        fn new(sha: String) -> Self {
            Self { sha }
        }
    }

    impl GitWorktree for FakeGit {
        fn resolve_sha(&self, _request: &ResolveGitShaRequest) -> Result<GitSha, String> {
            GitSha::new(self.sha.clone())
        }

        fn create(&self, request: &CreateChangeWorktreeRequest) -> Result<ChangeWorkspace, String> {
            Ok(
                ChangeWorkspace::new(
                    ChangeId::new("campaign-1".to_string()).unwrap(),
                    request.worktree_path().to_path_buf(),
                )
                .with_branch_name(request.branch_name().to_string())
                .with_base_sha(GitSha::new(self.sha.clone())?),
            )
        }

        fn inspect(&self, _request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String> {
            Err("inspect not used in this test".to_string())
        }
    }

    fn sample_campaign(base_sha: String) -> Campaign {
        Campaign {
            version: "rack-ai/campaign/v1".to_string(),
            campaign_id: "campaign-1".to_string(),
            repository: CampaignRepository {
                id: "adaptos".to_string(),
                base_ref: "main".to_string(),
                base_sha,
            },
            branch: "rack/campaign-campaign-1".to_string(),
            permitted_paths: vec!["src/".to_string()],
            allow_local_commits: true,
            limits: CampaignLimits {
                max_runtime_seconds: 600,
                max_steps: 2,
                max_total_attempts: 2,
                heartbeat_seconds: 30,
                network: "disabled".to_string(),
            },
            worker_policy: WorkerPolicy {
                primary: "local-coder".to_string(),
                fallback: "local-primary".to_string(),
                primary_attempts: 1,
                repair_attempts: 0,
                fallback_attempts: 0,
            },
            steps: vec![CampaignStep {
                id: "step-1".to_string(),
                kind: CampaignStepKind::Implementation,
                task: "Add the domain file.".to_string(),
                allowed_paths: vec!["src/".to_string()],
                required_changed_paths: vec!["src/".to_string()],
                acceptance: StepAcceptance {
                    commands: vec![vec!["cargo".to_string(), "test".to_string()]],
                    required_artifacts: vec!["src/lib.rs".to_string()],
                },
                limits: StepLimits {
                    timeout_seconds: 300,
                    network: "disabled".to_string(),
                },
            }],
        }
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-campaign-runner-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
