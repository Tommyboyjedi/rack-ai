use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rack_ai_domain::AcceptanceCommand;
use rack_ai_domain::ChangeId;
use rack_ai_domain::GitSha;
use rack_ai_domain::RepositoryId;

use crate::assert_campaign_git_args;
use crate::Campaign;
use crate::CampaignCommitRequest;
use crate::CampaignHealth;
use crate::CampaignLimits;
use crate::CampaignRepository;
use crate::CampaignRevisionDocument;
use crate::CampaignRunner;
use crate::CampaignRunnerDependencies;
use crate::CampaignState;
use crate::CampaignStep;
use crate::CampaignStepKind;
use crate::CampaignWorkerCatalog;
use crate::CampaignWorkerRuntime;
use crate::ChangeWorkspace;
use crate::CommandEvidence;
use crate::CommandPolicy;
use crate::CoordinatorReviewDisposition;
use crate::CreateChangeWorktreeRequest;
use crate::ExecutorConfig;
use crate::FailureClassification;
use crate::GitEvidence;
use crate::GitWorktree;
use crate::InspectChangeWorktreeRequest;
use crate::ReadFileRequest;
use crate::RegisteredRepository;
use crate::RepositoryRegistry;
use crate::ResolveGitShaRequest;
use crate::RunCommandRequest;
use crate::ScriptedAttempt;
use crate::ScriptedChangeImplementer;
use crate::ScriptedWrite;
use crate::StepAcceptance;
use crate::StepLimits;
use crate::UnixClock;
use crate::WorkerPolicy;
use crate::WorkspaceExecutionResult;
use crate::WorkspaceExecutor;
use crate::WorkspaceRoot;
use crate::WriteFileRequest;

struct TestClock {
    now: Cell<u64>,
}

impl UnixClock for TestClock {
    fn now_unix(&self) -> u64 {
        self.now.get()
    }
}

struct AllowAllPolicy;

impl CommandPolicy for AllowAllPolicy {
    fn assert_allowed(&self, _command: &AcceptanceCommand) -> Result<(), String> {
        Ok(())
    }
}

struct Healthy;

impl CampaignHealth for Healthy {
    fn assert_workers(&self, _primary: &str, _fallback: &str) -> Result<(), String> {
        Ok(())
    }
    fn assert_executor(&self) -> Result<(), String> {
        Ok(())
    }
}

struct UnhealthyExecutor;

impl CampaignHealth for UnhealthyExecutor {
    fn assert_workers(&self, _primary: &str, _fallback: &str) -> Result<(), String> {
        Ok(())
    }
    fn assert_executor(&self) -> Result<(), String> {
        Err("podman is not available".to_string())
    }
}

struct UnhealthyWorker;

impl CampaignHealth for UnhealthyWorker {
    fn assert_workers(&self, _primary: &str, _fallback: &str) -> Result<(), String> {
        Err("worker endpoint is unhealthy: local-coder".to_string())
    }
    fn assert_executor(&self) -> Result<(), String> {
        Ok(())
    }
}

struct StaticWorkers;

impl CampaignWorkerCatalog for StaticWorkers {
    fn runtime(&self, worker_id: &str) -> Result<CampaignWorkerRuntime, String> {
        if worker_id == "local-coder-jcode" {
            return Err("host-oriented JCode workers are rejected for campaigns".to_string());
        }
        Ok(CampaignWorkerRuntime {
            worker_id: worker_id.to_string(),
            endpoint: format!("http://127.0.0.1/{worker_id}"),
            api_model_id: worker_id.to_string(),
        })
    }
}

struct TestRegistry {
    repository: RegisteredRepository,
    workspace_root: WorkspaceRoot,
}

impl TestRegistry {
    fn new(repository_root: PathBuf, workspace_root: PathBuf) -> Self {
        Self {
            repository: RegisteredRepository::new(
                RepositoryId::new("fixture".to_string()).unwrap(),
                repository_root,
            )
            .unwrap(),
            workspace_root: WorkspaceRoot::new(workspace_root).unwrap(),
        }
    }
}

impl RepositoryRegistry for TestRegistry {
    fn workspace_root(&self) -> Result<WorkspaceRoot, String> {
        Ok(self.workspace_root.clone())
    }
    fn executor_config(&self) -> Result<ExecutorConfig, String> {
        ExecutorConfig::podman("docker.io/library/rust:bookworm".to_string())
    }
    fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
        if self.repository.id() != id {
            return Err("not registered".to_string());
        }
        Ok(self.repository.clone())
    }
}

struct ProcessGit;

static PROCESS_GIT: ProcessGit = ProcessGit;
static ALLOW_ALL: AllowAllPolicy = AllowAllPolicy;
static WORKERS: StaticWorkers = StaticWorkers;

impl GitWorktree for ProcessGit {
    fn resolve_sha(&self, request: &ResolveGitShaRequest) -> Result<GitSha, String> {
        GitSha::new(git(
            request.repository_root(),
            &["rev-parse", request.git_ref().value()],
        )?)
    }
    fn create(&self, request: &CreateChangeWorktreeRequest) -> Result<ChangeWorkspace, String> {
        assert_campaign_git_args(&[
            "worktree",
            "add",
            "-b",
            request.branch_name(),
            request.worktree_path().to_str().unwrap(),
            request.base_sha().value(),
        ])?;
        if let Some(parent) = request.worktree_path().parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        git(
            request.repository_root(),
            &[
                "worktree",
                "add",
                "-b",
                request.branch_name(),
                request.worktree_path().to_str().unwrap(),
                request.base_sha().value(),
            ],
        )?;
        Ok(ChangeWorkspace::new(
            ChangeId::new("campaign".to_string()).unwrap(),
            request.worktree_path().to_path_buf(),
        )
        .with_branch_name(request.branch_name().to_string())
        .with_base_sha(request.base_sha().clone()))
    }
    fn inspect(&self, request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String> {
        let evidence = self.snapshot(request.worktree_path())?;
        if evidence.head_sha() != request.expected_base_sha() {
            return Err("worktree is not at the recorded base sha".to_string());
        }
        Ok(evidence)
    }
    fn snapshot(&self, worktree_path: &Path) -> Result<GitEvidence, String> {
        let head = GitSha::new(git(worktree_path, &["rev-parse", "HEAD"])?)?;
        let status = git(worktree_path, &["status", "--porcelain"])?;
        let diff = git(worktree_path, &["diff"])?;
        let diff_stat = git(worktree_path, &["diff", "--stat"])?;
        let changed = porcelain_paths(&status);
        Ok(GitEvidence::new(head, status)
            .with_diff(diff)
            .with_diff_stat(diff_stat)
            .with_changed_paths(changed))
    }
    fn current_branch(&self, worktree_path: &Path) -> Result<String, String> {
        git(worktree_path, &["branch", "--show-current"])
    }
    fn current_head(&self, worktree_path: &Path) -> Result<GitSha, String> {
        GitSha::new(git(worktree_path, &["rev-parse", "HEAD"])?)
    }
    fn commit_local(&self, request: &CampaignCommitRequest) -> Result<GitSha, String> {
        let mut add = vec!["add", "--"];
        let paths: Vec<&str> = request.paths().iter().map(String::as_str).collect();
        add.extend(paths.iter().copied());
        assert_campaign_git_args(&add)?;
        git(request.worktree_path(), &add)?;
        let name = format!("user.name={}", request.author_name());
        let email = format!("user.email={}", request.author_email());
        let commit = [
            "-c",
            name.as_str(),
            "-c",
            email.as_str(),
            "commit",
            "-m",
            request.message(),
        ];
        assert_campaign_git_args(&commit)?;
        git(request.worktree_path(), &commit)?;
        GitSha::new(git(request.worktree_path(), &["rev-parse", "HEAD"])?)
    }
}

struct HostExecutor {
    writes: Mutex<Vec<String>>,
    poison_path: Option<String>,
    read_error: Option<String>,
}

impl HostExecutor {
    fn new() -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            poison_path: None,
            read_error: None,
        }
    }
    fn with_poison(path: &str) -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            poison_path: Some(path.to_string()),
            read_error: None,
        }
    }
    fn with_read_error(error: &str) -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            poison_path: None,
            read_error: Some(error.to_string()),
        }
    }
}

impl WorkspaceExecutor for HostExecutor {
    fn write_file(&self, request: &WriteFileRequest) -> Result<WorkspaceExecutionResult, String> {
        let path = request.worktree_path().join(request.path().relative());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, request.content()).map_err(|error| error.to_string())?;
        self.writes
            .lock()
            .unwrap()
            .push(request.path().relative().to_string());
        Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
            vec!["write".to_string(), request.path().relative().to_string()],
            0,
        )))
    }
    fn read_file(&self, request: &ReadFileRequest) -> Result<WorkspaceExecutionResult, String> {
        if let Some(error) = &self.read_error {
            return Err(error.clone());
        }
        let path = request.worktree_path().join(request.path().relative());
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        Ok(
            WorkspaceExecutionResult::new(CommandEvidence::new(vec!["read".to_string()], 0))
                .with_content(content),
        )
    }
    fn run_command(&self, request: &RunCommandRequest) -> Result<WorkspaceExecutionResult, String> {
        if let Some(poison) = &self.poison_path {
            let path = request.worktree_path().join(poison);
            fs::write(path, "poison\n").map_err(|error| error.to_string())?;
        }
        let failed = request.argv().iter().any(|item| item == "FAIL");
        Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
            request.argv().to_vec(),
            if failed { 1 } else { 0 },
        )))
    }
}

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    workspaces: PathBuf,
    sha: String,
}

fn fixture() -> Fixture {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rack-ai-campaign-flow-{nanos}"));
    let repo = root.join("repo");
    let workspaces = root.join("workspaces");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    git_init(&repo);
    let sha = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    Fixture {
        root,
        repo,
        workspaces,
        sha,
    }
}

fn porcelain_paths(status: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in status.lines() {
        if line.len() < 4 {
            continue;
        }
        let path_part = line[3..].trim();
        let path = path_part
            .rsplit_once(" -> ")
            .map(|(_, dest)| dest)
            .unwrap_or(path_part)
            .trim_matches('"');
        if !path.is_empty() && !paths.iter().any(|existing| existing == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

fn git_init(repo: &Path) {
    assert!(Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .status()
        .unwrap()
        .success());
    git(repo, &["config", "user.email", "test@example.com"]).unwrap();
    git(repo, &["config", "user.name", "test"]).unwrap();
    git(repo, &["add", "."]).unwrap();
    git(repo, &["commit", "-m", "init"]).unwrap();
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

fn sample_step(id: &str, path: &str, task: &str) -> CampaignStep {
    CampaignStep {
        id: id.to_string(),
        kind: CampaignStepKind::Implementation,
        task: task.to_string(),
        allowed_paths: vec!["src/".to_string()],
        required_changed_paths: vec![path.to_string()],
        acceptance: StepAcceptance {
            commands: vec![vec!["true".to_string()]],
            required_artifacts: vec![path.to_string()],
        },
        limits: StepLimits {
            timeout_seconds: 60,
            network: "disabled".to_string(),
        },
    }
}

fn make_campaign(id: &str, sha: &str, steps: Vec<CampaignStep>, policy: WorkerPolicy) -> Campaign {
    Campaign {
        version: "rack-ai/campaign/v1".to_string(),
        campaign_id: id.to_string(),
        repository: CampaignRepository {
            id: "fixture".to_string(),
            base_ref: "main".to_string(),
            base_sha: sha.to_string(),
        },
        branch: format!("rack/campaign-{id}"),
        permitted_paths: vec!["src/".to_string()],
        allow_local_commits: true,
        limits: CampaignLimits {
            max_runtime_seconds: 600,
            max_steps: 8,
            max_total_attempts: 8,
            heartbeat_seconds: 10,
            network: "disabled".to_string(),
        },
        worker_policy: policy,
        steps,
    }
}

fn default_policy() -> WorkerPolicy {
    WorkerPolicy {
        primary: "local-coder".to_string(),
        fallback: "local-primary".to_string(),
        primary_attempts: 1,
        repair_attempts: 1,
        fallback_attempts: 1,
    }
}

fn write_attempt(path: &str, content: &str) -> ScriptedAttempt {
    ScriptedAttempt {
        match_worker: None,
        writes: vec![ScriptedWrite {
            path: path.to_string(),
            content: content.to_string(),
        }],
        output: "COMPLETE".to_string(),
        error: None,
        protocol_error: None,
        executor_kind: None,
    }
}

fn empty_attempt(output: &str) -> ScriptedAttempt {
    ScriptedAttempt {
        match_worker: None,
        writes: Vec::new(),
        output: output.to_string(),
        error: None,
        protocol_error: None,
        executor_kind: None,
    }
}

#[test]
fn two_step_campaign_creates_two_local_commits() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt("src/alpha.rs", "pub fn alpha() -> u8 { 1 }\n"),
            write_attempt("src/beta.rs", "pub fn beta() -> u8 { 2 }\n"),
        ],
    );
    let campaign = make_campaign(
        "two-step",
        &fx.sha,
        vec![
            sample_step("add-alpha", "src/alpha.rs", "Add alpha."),
            sample_step("add-beta", "src/beta.rs", "Add beta."),
        ],
        default_policy(),
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(state.state, CampaignState::Completed);
    let first = state.steps[0].accepted_commit.clone().unwrap();
    let second = state.steps[1].accepted_commit.clone().unwrap();
    assert_ne!(first, second);
    assert_eq!(state.current_head_sha, second);
    let parent = git(Path::new(&state.worktree_path), &["rev-parse", "HEAD^"]).unwrap();
    assert_eq!(parent, first);
    let default_head = git(&fx.repo, &["rev-parse", "main"]).unwrap();
    assert_eq!(default_head, fx.sha);
    let branch = git(
        Path::new(&state.worktree_path),
        &["branch", "--show-current"],
    )
    .unwrap();
    assert_eq!(branch, "rack/campaign-two-step");
    let msg = git(
        Path::new(&state.worktree_path),
        &["log", "-1", "--format=%s"],
    )
    .unwrap();
    assert_eq!(msg, "rack(two-step): add-beta");
}

#[test]
fn complete_without_diff_is_no_change() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, vec![empty_attempt("COMPLETE")]);
    let campaign = make_campaign(
        "noop",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::NoChange)
    );
    assert!(state.steps[0].accepted_commit.is_none());
}

#[test]
fn markdown_tool_call_is_protocol_violation() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![ScriptedAttempt {
            match_worker: None,
            writes: Vec::new(),
            output: "```json\n{\"name\": \"write\", \"arguments\": {\"file_path\": \"src/alpha.rs\"}}\n```\nCOMPLETE".to_string(),
            error: None,
            protocol_error: None,
            executor_kind: None,
        }],
    );
    let campaign = make_campaign(
        "md-tools",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::ToolProtocolViolation)
    );
}

#[test]
fn artifact_executor_failure_fails_closed_without_repair_or_fallback() {
    let fx = fixture();
    let executor = HostExecutor::with_read_error(
        "podman is not available; rootless Podman is required for external-repository command execution",
    );
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt("src/alpha.rs", "pub fn alpha() -> u8 { 1 }\n"),
            write_attempt("src/alpha.rs", "should not run"),
            write_attempt("src/alpha.rs", "should not run either"),
        ],
    );
    let campaign = make_campaign(
        "artifact-executor",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(
        state.blocked_reason.as_deref(),
        Some("executor_unavailable")
    );
    assert!(
        state.steps[0].attempts.len() <= 1,
        "executor failure must not burn repair/fallback attempts: {:?}",
        state.steps[0].attempts
    );
    assert_eq!(implementer.seen_workers(), ["local-coder"]);
}

#[test]
fn post_check_out_of_policy_write_rejects_without_commit() {
    let fx = fixture();
    let executor = HostExecutor::with_poison("README.md");
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }\n",
        )],
    );
    let campaign = make_campaign(
        "policy",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::PathPolicyFailed)
    );
    assert!(state.steps[0].accepted_commit.is_none());
    assert_eq!(state.steps[0].attempts.len(), 1);
}

#[test]
fn missing_required_changed_path_is_rejected() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt("src/lib.rs", "pub fn value() -> u8 { 2 }\n")],
    );
    let campaign = make_campaign(
        "missing-required",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::NoChange),
        "{:?}",
        state.steps[0].attempts[0]
    );
    assert_eq!(
        state.steps[0].review_disposition,
        Some(CoordinatorReviewDisposition::RejectedRetryable)
    );
}

#[test]
fn repair_and_fallback_bounds_then_stop() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            empty_attempt("COMPLETE"),
            empty_attempt("COMPLETE"),
            empty_attempt("COMPLETE"),
            write_attempt("src/alpha.rs", "should not run"),
        ],
    );
    let campaign = make_campaign(
        "bounds",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(state.steps[0].attempts.len(), 3);
    assert_eq!(state.steps[0].attempts[0].worker_id, "local-coder");
    assert_eq!(state.steps[0].attempts[1].worker_id, "local-coder");
    assert_eq!(state.steps[0].attempts[2].worker_id, "local-primary");
    assert_eq!(state.state, CampaignState::Blocked);
    assert!(state.steps[0].attempts[1]
        .repair_instruction
        .as_ref()
        .unwrap()
        .contains("Do not broaden"));
}

#[test]
fn fallback_uses_workspace_executor_not_host_jcode() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            empty_attempt("COMPLETE"),
            empty_attempt("COMPLETE"),
            ScriptedAttempt {
                match_worker: Some("local-primary".to_string()),
                writes: vec![ScriptedWrite {
                    path: "src/alpha.rs".to_string(),
                    content: "pub fn alpha() -> u8 { 1 }\n".to_string(),
                }],
                output: "COMPLETE".to_string(),
                error: None,
                protocol_error: None,
                executor_kind: Some("workspace".to_string()),
            },
        ],
    );
    let campaign = make_campaign(
        "fallback-exec",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(state.state, CampaignState::Completed);
    assert!(executor
        .writes
        .lock()
        .unwrap()
        .contains(&"src/alpha.rs".to_string()));
    assert_eq!(
        implementer.seen_workers(),
        ["local-coder", "local-coder", "local-primary"]
    );
}

#[test]
fn inadequate_change_with_passing_tests_is_retryable_review() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/lib.rs",
            "// not the requested domain change\n",
        )],
    );
    let mut step = sample_step("domain", "src/domain/mod.rs", "Add domain identifiers.");
    step.required_changed_paths = vec!["src/domain/".to_string()];
    let campaign = make_campaign(
        "inadequate",
        &fx.sha,
        vec![step],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    assert_eq!(
        state.steps[0].review_disposition,
        Some(CoordinatorReviewDisposition::RejectedRetryable)
    );
    assert!(state.steps[0].accepted_commit.is_none());
    let packet = fs::read_to_string(
        fx.root
            .join("state/campaigns/inadequate/steps/domain/attempt-1/review-packet.json"),
    )
    .unwrap();
    assert!(packet.contains("rejected_retryable"));
}

#[test]
fn recovery_after_accepted_step_does_not_repeat_commit() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt("src/alpha.rs", "pub fn alpha() -> u8 { 1 }\n"),
            write_attempt("src/beta.rs", "pub fn beta() -> u8 { 2 }\n"),
        ],
    );
    let campaign = make_campaign(
        "recover",
        &fx.sha,
        vec![
            sample_step("add-alpha", "src/alpha.rs", "Add alpha."),
            sample_step("add-beta", "src/beta.rs", "Add beta."),
        ],
        default_policy(),
    );
    let registry = TestRegistry::new(fx.repo.clone(), fx.workspaces.clone());
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let runner = CampaignRunner::new(CampaignRunnerDependencies {
        registry: &registry,
        command_policy: &ALLOW_ALL,
        git: &PROCESS_GIT,
        implementer: &implementer,
        executor: &executor,
        workers: &WORKERS,
        health: &Healthy,
        clock: &clock,
        state_root: fx.root.clone(),
    });
    runner.start(&campaign).unwrap();
    runner.run("recover").unwrap();
    let after_first = runner.load_state("recover").unwrap().unwrap();
    assert_eq!(after_first.steps[0].disposition, "accepted");
    let first_commit = after_first.steps[0].accepted_commit.clone().unwrap();
    let again = runner.run("recover").unwrap();
    assert_eq!(
        again.steps[0].accepted_commit.as_deref(),
        Some(first_commit.as_str())
    );
    assert_eq!(again.steps[0].attempts.len(), 1);
    assert_eq!(again.state, CampaignState::Completed);
}

#[test]
fn detach_setup_failure_blocks_created_campaign() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let campaign = make_campaign(
        "detach-fail",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    let blocked = runner
        .mark_detach_setup_failed("detach-fail", "user-level systemd is required")
        .unwrap();
    assert_eq!(blocked.state, CampaignState::Blocked);
    assert_eq!(
        blocked.blocked_reason.as_deref(),
        Some("executor_unavailable")
    );
    assert!(blocked
        .error_message
        .unwrap()
        .contains("detached runner setup failed"));
}

#[test]
fn pause_blocks_next_action_and_resume_continues() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt("src/alpha.rs", "pub fn alpha() -> u8 { 1 }\n"),
            write_attempt("src/beta.rs", "pub fn beta() -> u8 { 2 }\n"),
        ],
    );
    let campaign = make_campaign(
        "pause",
        &fx.sha,
        vec![
            sample_step("add-alpha", "src/alpha.rs", "Add alpha."),
            sample_step("add-beta", "src/beta.rs", "Add beta."),
        ],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    runner.pause("pause").unwrap();
    let paused = runner.run("pause").unwrap();
    assert_eq!(paused.state, CampaignState::Paused);
    assert!(paused.steps.iter().all(|step| step.attempts.is_empty()));
    let mut state = runner.load_state("pause").unwrap().unwrap();
    state.pause_requested = false;
    state.state = CampaignState::Running;
    runner.save_state(&state).unwrap();
    let resumed = runner.run("pause").unwrap();
    assert_eq!(resumed.state, CampaignState::Completed);
}

#[test]
fn revision_appends_steps_without_rewriting_accepted_history() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt("src/alpha.rs", "pub fn alpha() -> u8 { 1 }\n"),
            write_attempt("src/beta.rs", "pub fn beta() -> u8 { 2 }\n"),
        ],
    );
    let campaign = make_campaign(
        "revise",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    let completed = runner.run("revise").unwrap();
    let accepted = completed.steps[0].clone();
    let mut state = runner.load_state("revise").unwrap().unwrap();
    state.state = CampaignState::Paused;
    runner.save_state(&state).unwrap();
    runner
        .revise(
            "revise",
            CampaignRevisionDocument {
                instruction: "Add beta next.".to_string(),
                steps: vec![sample_step("add-beta", "src/beta.rs", "Add beta.")],
            },
        )
        .unwrap();
    let after = runner.run("revise").unwrap();
    assert_eq!(after.steps[0].accepted_commit, accepted.accepted_commit);
    assert_eq!(after.steps[0].attempts.len(), accepted.attempts.len());
    assert_eq!(after.steps[1].disposition, "accepted");
}

#[test]
fn cancel_prevents_commit_and_retains_evidence() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }\n",
        )],
    );
    let campaign = make_campaign(
        "cancel",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    runner.cancel("cancel", Some("stop")).unwrap();
    let state = runner.run("cancel").unwrap();
    assert_eq!(state.state, CampaignState::Cancelled);
    assert!(state.steps[0].accepted_commit.is_none());
    assert!(runner.campaign_dir("cancel").join("campaign.json").exists());
}

#[test]
fn fail_closed_on_expiry_lease_digest_dirty_executor_and_worker() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer =
        ScriptedChangeImplementer::new(&executor, vec![write_attempt("src/alpha.rs", "x\n")]);
    let mut closed = make_campaign(
        "closed",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    closed.limits.max_runtime_seconds = 60;
    let runner = make_runner(&fx, &closed, &implementer, &executor, &Healthy, 1_000);
    runner.start(&closed).unwrap();
    let clock = TestClock {
        now: Cell::new(1_070),
    };
    let expired = CampaignRunner::new(CampaignRunnerDependencies {
        registry: &TestRegistry::new(fx.repo.clone(), fx.workspaces.clone()),
        command_policy: &ALLOW_ALL,
        git: &PROCESS_GIT,
        implementer: &implementer,
        executor: &executor,
        workers: &WORKERS,
        health: &Healthy,
        clock: &clock,
        state_root: fx.root.clone(),
    })
    .run("closed")
    .unwrap();
    assert_eq!(expired.state, CampaignState::Expired);

    let fx = fixture();
    let implementer =
        ScriptedChangeImplementer::new(&executor, vec![write_attempt("src/alpha.rs", "x\n")]);
    let campaign_doc = make_campaign(
        "digest",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign_doc, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign_doc).unwrap();
    let mut state = runner.load_state("digest").unwrap().unwrap();
    state.campaign_digest = "deadbeef".to_string();
    runner.save_state(&state).unwrap();
    let saved = runner.load_state("digest").unwrap().unwrap();
    assert_eq!(saved.campaign_digest, "deadbeef");
    let blocked = runner.run("digest").unwrap();
    assert_eq!(
        blocked.state,
        CampaignState::Blocked,
        "digest={} error={:?} reason={:?}",
        blocked.campaign_digest,
        blocked.error_message,
        blocked.blocked_reason
    );
    assert_eq!(blocked.blocked_reason.as_deref(), Some("continuity_failed"));

    let fx = fixture();
    let implementer =
        ScriptedChangeImplementer::new(&executor, vec![write_attempt("src/alpha.rs", "x\n")]);
    let campaign = make_campaign(
        "dirty",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    let started = runner.start(&campaign).unwrap();
    fs::write(
        Path::new(&started.worktree_path).join("src/lib.rs"),
        "dirty\n",
    )
    .unwrap();
    let blocked = runner.run("dirty").unwrap();
    assert_eq!(blocked.blocked_reason.as_deref(), Some("continuity_failed"));

    let fx = fixture();
    let campaign = make_campaign(
        "exec",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &UnhealthyExecutor,
        1_000,
    );
    let error = runner.start(&campaign).unwrap_err();
    assert!(error.contains("podman"));

    let fx = fixture();
    let campaign = make_campaign(
        "worker",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &UnhealthyWorker,
        1_000,
    );
    let error = runner.start(&campaign).unwrap_err();
    assert!(error.contains("unhealthy"));
}

#[test]
fn production_campaign_git_path_has_no_remote_operations() {
    assert!(assert_campaign_git_args(&["push", "origin", "main"]).is_err());
    assert!(assert_campaign_git_args(&["fetch"]).is_err());
    let source = include_str!("campaign_runner.rs");
    assert!(!source.contains("reset --hard"));
    assert!(!source.contains("clean -fd"));
    assert!(!source.contains("\"push\""));
}

fn run_campaign(
    fx: &Fixture,
    campaign: &Campaign,
    implementer: &ScriptedChangeImplementer<'_>,
    executor: &HostExecutor,
    health: &dyn CampaignHealth,
    now: u64,
) -> crate::CampaignStatus {
    let runner = make_runner(fx, campaign, implementer, executor, health, now);
    runner.start(campaign).unwrap();
    runner.run(&campaign.campaign_id).unwrap()
}

fn make_runner<'a>(
    fx: &'a Fixture,
    _campaign: &Campaign,
    implementer: &'a ScriptedChangeImplementer<'a>,
    executor: &'a HostExecutor,
    health: &'a dyn CampaignHealth,
    now: u64,
) -> CampaignRunner<'a> {
    CampaignRunner::new(CampaignRunnerDependencies {
        registry: Box::leak(Box::new(TestRegistry::new(
            fx.repo.clone(),
            fx.workspaces.clone(),
        ))),
        command_policy: &ALLOW_ALL,
        git: &PROCESS_GIT,
        implementer,
        executor,
        workers: &WORKERS,
        health,
        clock: Box::leak(Box::new(TestClock {
            now: Cell::new(now),
        })),
        state_root: fx.root.clone(),
    })
}
