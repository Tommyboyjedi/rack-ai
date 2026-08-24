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

use crate::AcceptingReviewer;
use crate::AttemptKind;
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
use crate::CampaignSupervisor;
use crate::CampaignSupervisorDependencies;
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
use crate::ImplementationReviewer;
use crate::InspectChangeWorktreeRequest;
use crate::ModelReviewRequest;
use crate::ModelReviewResult;
use crate::OperationsConfig;
use crate::ReadFileRequest;
use crate::RecoveryDecision;
use crate::RecoveryDecisionKind;
use crate::RecoveryFailureKind;
use crate::RecoveryReasoner;
use crate::RecoveryReasoningRequest;
use crate::RecoveryReasoningResult;
use crate::RecoverySleeper;
use crate::RecoveryWorkerAction;
use crate::RegisteredRepository;
use crate::RepositoryRegistry;
use crate::ResolveGitShaRequest;
use crate::RetentionConfig;
use crate::RunCommandRequest;
use crate::ScriptedAttempt;
use crate::ScriptedChangeImplementer;
use crate::ScriptedWrite;
use crate::StepAcceptance;
use crate::StepLimits;
use crate::SupervisorConfig;
use crate::UnixClock;
use crate::WorkerPolicy;
use crate::WorkspaceExecutionResult;
use crate::WorkspaceExecutor;
use crate::WorkspaceRoot;
use crate::WriteFileRequest;
use crate::assert_campaign_git_args;

struct TestClock {
    now: Cell<u64>,
}

impl UnixClock for TestClock {
    fn now_unix(&self) -> u64 {
        self.now.get()
    }
}

struct NoopSleeper;

impl RecoverySleeper for NoopSleeper {
    fn sleep_seconds(&self, _seconds: u64) {}
}

struct RecordingSleeper<'a> {
    clock: &'a TestClock,
    sleeps: Mutex<Vec<u64>>,
}

impl<'a> RecordingSleeper<'a> {
    fn new(clock: &'a TestClock) -> Self {
        Self {
            clock,
            sleeps: Mutex::new(Vec::new()),
        }
    }

    fn sleeps(&self) -> Vec<u64> {
        self.sleeps.lock().unwrap().clone()
    }
}

impl RecoverySleeper for RecordingSleeper<'_> {
    fn sleep_seconds(&self, seconds: u64) {
        self.sleeps.lock().unwrap().push(seconds);
        self.clock
            .now
            .set(self.clock.now.get().saturating_add(seconds));
    }
}

struct PauseRequestSleeper<'a> {
    clock: &'a TestClock,
    state_root: PathBuf,
    campaign_id: String,
    triggered: Cell<bool>,
}

impl<'a> PauseRequestSleeper<'a> {
    fn new(clock: &'a TestClock, state_root: PathBuf, campaign_id: &str) -> Self {
        Self {
            clock,
            state_root,
            campaign_id: campaign_id.to_string(),
            triggered: Cell::new(false),
        }
    }
}

impl RecoverySleeper for PauseRequestSleeper<'_> {
    fn sleep_seconds(&self, seconds: u64) {
        self.clock
            .now
            .set(self.clock.now.get().saturating_add(seconds));
        if self.triggered.replace(true) {
            return;
        }
        let path = self
            .state_root
            .join("state")
            .join("campaigns")
            .join(&self.campaign_id)
            .join("state.json");
        let mut value =
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap();
        value["pause_requested"] = serde_json::Value::Bool(true);
        fs::write(
            &path,
            serde_json::to_string_pretty(&value).unwrap()
                + "
",
        )
        .unwrap();
    }
}

struct FlakyWorkerHealth {
    startup_successes: Cell<usize>,
    failures: Mutex<Vec<String>>,
}

impl FlakyWorkerHealth {
    fn new(startup_successes: usize, failures: Vec<&str>) -> Self {
        Self {
            startup_successes: Cell::new(startup_successes),
            failures: Mutex::new(failures.into_iter().map(str::to_string).collect()),
        }
    }
}

impl CampaignHealth for FlakyWorkerHealth {
    fn assert_workers(&self, _primary: &str, _fallback: &str) -> Result<(), String> {
        self.assert_worker("local-primary")
    }

    fn assert_worker(&self, _worker_id: &str) -> Result<(), String> {
        let remaining_successes = self.startup_successes.get();
        if remaining_successes > 0 {
            self.startup_successes.set(remaining_successes - 1);
            return Ok(());
        }
        let mut failures = self.failures.lock().unwrap();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.remove(0))
        }
    }

    fn assert_executor(&self) -> Result<(), String> {
        Ok(())
    }
}

struct AllowAllPolicy;

impl CommandPolicy for AllowAllPolicy {
    fn assert_allowed(&self, _command: &AcceptanceCommand) -> Result<(), String> {
        Ok(())
    }
}

struct ResumeCurrentWorkerHealthyOtherUnavailable {
    coder_failures_left: Cell<usize>,
}

impl ResumeCurrentWorkerHealthyOtherUnavailable {
    fn new(coder_failures_left: usize) -> Self {
        Self {
            coder_failures_left: Cell::new(coder_failures_left),
        }
    }
}

impl CampaignHealth for ResumeCurrentWorkerHealthyOtherUnavailable {
    fn assert_workers(&self, _primary: &str, _fallback: &str) -> Result<(), String> {
        Err("worker endpoint is unhealthy: local-primary".to_string())
    }

    fn assert_worker(&self, worker_id: &str) -> Result<(), String> {
        if worker_id == "local-coder" {
            let remaining = self.coder_failures_left.get();
            if remaining > 0 {
                self.coder_failures_left.set(remaining - 1);
                return Err("io: Peer disconnected".to_string());
            }
            return Ok(());
        }
        Err(format!("worker endpoint is unhealthy: {worker_id}"))
    }

    fn assert_executor(&self) -> Result<(), String> {
        Ok(())
    }
}

struct Healthy;

impl CampaignHealth for Healthy {
    fn assert_workers(&self, _primary: &str, _fallback: &str) -> Result<(), String> {
        Ok(())
    }
    fn assert_worker(&self, _worker_id: &str) -> Result<(), String> {
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
    fn assert_worker(&self, _worker_id: &str) -> Result<(), String> {
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
    fn assert_worker(&self, _worker_id: &str) -> Result<(), String> {
        Err("worker endpoint is unhealthy: local-coder".to_string())
    }
    fn assert_executor(&self) -> Result<(), String> {
        Ok(())
    }
}

struct StaticWorkers;

impl CampaignWorkerCatalog for StaticWorkers {
    fn runtime(&self, worker_id: &str) -> Result<CampaignWorkerRuntime, String> {
        Ok(CampaignWorkerRuntime {
            worker_id: worker_id.to_string(),
            endpoint: format!("http://127.0.0.1/{worker_id}"),
            api_model_id: worker_id.to_string(),
            entrypoint: "/tmp/fake-jcode".to_string(),
            provider_profile: worker_id.to_string(),
            tool_profile: None,
            context_window: Some(16368),
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
static NOOP_SLEEPER: NoopSleeper = NoopSleeper;

struct FailingReviewer;
static FAILING_REVIEWER: FailingReviewer = FailingReviewer;

impl ImplementationReviewer for FailingReviewer {
    fn review(&self, _request: &ModelReviewRequest) -> Result<ModelReviewResult, String> {
        Err("review endpoint timed out".to_string())
    }
}

struct ScriptedReviewer {
    responses: Mutex<Vec<Result<ModelReviewResult, String>>>,
    calls: Cell<usize>,
}

struct ScriptedRecoveryReasoner {
    responses: Mutex<Vec<Result<RecoveryDecision, String>>>,
    calls: Cell<usize>,
    requests: Mutex<Vec<RecoveryReasoningRequest>>,
}

impl ScriptedRecoveryReasoner {
    fn new(responses: Vec<Result<RecoveryDecision, String>>) -> Self {
        Self {
            responses: Mutex::new(responses),
            calls: Cell::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.get()
    }

    fn requests(&self) -> Vec<RecoveryReasoningRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl ScriptedReviewer {
    fn new(responses: Vec<Result<ModelReviewResult, String>>) -> Self {
        Self {
            responses: Mutex::new(responses),
            calls: Cell::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.get()
    }
}

impl ImplementationReviewer for ScriptedReviewer {
    fn review(&self, _request: &ModelReviewRequest) -> Result<ModelReviewResult, String> {
        self.calls.set(self.calls.get() + 1);
        self.responses.lock().unwrap().remove(0)
    }
}

impl RecoveryReasoner for ScriptedRecoveryReasoner {
    fn diagnose(
        &self,
        request: &RecoveryReasoningRequest,
    ) -> Result<RecoveryReasoningResult, String> {
        self.calls.set(self.calls.get() + 1);
        self.requests.lock().unwrap().push(request.clone());
        match self.responses.lock().unwrap().remove(0) {
            Ok(decision) => Ok(RecoveryReasoningResult {
                raw_output: serde_json::to_string(&decision).unwrap(),
                prompt: request.prompt(),
                decision,
            }),
            Err(error) => Err(error),
        }
    }
}

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

    fn reset_managed_worktree(
        &self,
        worktree_path: &Path,
        expected_head: &GitSha,
        dirty_paths: &[String],
    ) -> Result<(), String> {
        let current = GitSha::new(git(worktree_path, &["rev-parse", "HEAD"])?)?;
        if &current != expected_head {
            return Err("worktree HEAD changed before managed reset".to_string());
        }
        for relative in dirty_paths {
            let path = worktree_path.join(relative);
            if path.is_dir() {
                fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
            } else if path.exists() {
                fs::remove_file(&path).map_err(|error| error.to_string())?;
            }
        }
        git(worktree_path, &["reset", "--hard", expected_head.value()])?;
        Ok(())
    }
}

struct HostExecutor {
    writes: Mutex<Vec<String>>,
    poison_path: Option<String>,
    read_error: Option<String>,
    command_stdout: Option<String>,
    command_stderr: Option<String>,
    command_exit_code: i32,
    passthrough_commands: bool,
}

impl HostExecutor {
    fn new() -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            poison_path: None,
            read_error: None,
            command_stdout: None,
            command_stderr: None,
            command_exit_code: 0,
            passthrough_commands: false,
        }
    }
    fn with_process_commands() -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            poison_path: None,
            read_error: None,
            command_stdout: None,
            command_stderr: None,
            command_exit_code: 0,
            passthrough_commands: true,
        }
    }

    fn with_poison(path: &str) -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            poison_path: Some(path.to_string()),
            read_error: None,
            command_stdout: None,
            command_stderr: None,
            command_exit_code: 0,
            passthrough_commands: false,
        }
    }
    fn with_read_error(error: &str) -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            poison_path: None,
            read_error: Some(error.to_string()),
            command_stdout: None,
            command_stderr: None,
            command_exit_code: 0,
            passthrough_commands: false,
        }
    }

    fn with_command_failure(stdout: &str, stderr: &str, exit_code: i32) -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            poison_path: None,
            read_error: None,
            command_stdout: Some(stdout.to_string()),
            command_stderr: Some(stderr.to_string()),
            command_exit_code: exit_code,
            passthrough_commands: false,
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
        if self.passthrough_commands {
            let output = Command::new(&request.argv()[0])
                .args(&request.argv()[1..])
                .current_dir(request.worktree_path())
                .output()
                .map_err(|error| error.to_string())?;
            let code = output.status.code().unwrap_or(1);
            let evidence = CommandEvidence::new(request.argv().to_vec(), code)
                .with_stdout(String::from_utf8_lossy(&output.stdout).to_string())
                .with_stderr(String::from_utf8_lossy(&output.stderr).to_string());
            return Ok(WorkspaceExecutionResult::new(evidence));
        }
        if let Some(poison) = &self.poison_path {
            let path = request.worktree_path().join(poison);
            fs::write(path, "poison\n").map_err(|error| error.to_string())?;
        }
        let failed =
            request.argv().iter().any(|item| item == "FAIL") || self.command_exit_code != 0;
        let mut evidence = CommandEvidence::new(
            request.argv().to_vec(),
            if failed {
                self.command_exit_code.max(1)
            } else {
                0
            },
        );
        if let Some(stdout) = &self.command_stdout {
            evidence = evidence.with_stdout(stdout.clone());
        }
        if let Some(stderr) = &self.command_stderr {
            evidence = evidence.with_stderr(stderr.clone());
        }
        Ok(WorkspaceExecutionResult::new(evidence))
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

fn cargo_fixture(service_source: &str) -> Fixture {
    let fx = fixture();
    let cargo = r#"[package]
name = "fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
"#;
    let main = r#"mod service;

use service::AssessmentService;

fn main() {
    let service = AssessmentService;
    println!("{}", service.open_case());
}
"#;
    fs::write(fx.repo.join("Cargo.toml"), cargo).unwrap();
    fs::write(fx.repo.join("src/main.rs"), main).unwrap();
    fs::write(fx.repo.join("src/service.rs"), service_source).unwrap();
    let _ = fs::remove_file(fx.repo.join("src/lib.rs"));
    assert!(
        Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(&fx.repo)
            .status()
            .unwrap()
            .success()
    );
    git(&fx.repo, &["add", "."]).unwrap();
    git(&fx.repo, &["commit", "-m", "cargo fixture"]).unwrap();
    let sha = git(&fx.repo, &["rev-parse", "HEAD"]).unwrap();
    Fixture { sha, ..fx }
}

fn compatibility_service_source() -> &'static str {
    r#"pub struct AssessmentService;

impl AssessmentService {
    pub fn open_case(&self) -> u32 {
        1
    }
}
"#
}

fn compatibility_step(task: &str) -> CampaignStep {
    CampaignStep {
        id: "service".to_string(),
        kind: CampaignStepKind::Implementation,
        task: task.to_string(),
        allowed_paths: vec!["src/service.rs".to_string()],
        required_changed_paths: vec!["src/service.rs".to_string()],
        acceptance: StepAcceptance {
            commands: vec![vec![
                "cargo".to_string(),
                "check".to_string(),
                "--offline".to_string(),
            ]],
            required_artifacts: vec!["src/service.rs".to_string()],
        },
        limits: StepLimits {
            timeout_seconds: 60,
            network: "disabled".to_string(),
        },
    }
}

fn decision(
    kind: RecoveryDecisionKind,
    failure_kind: RecoveryFailureKind,
    worker_action: RecoveryWorkerAction,
    next_instruction: Option<&str>,
) -> RecoveryDecision {
    RecoveryDecision {
        kind,
        failure_kind,
        rationale: "diagnosed by scripted recovery reasoner".to_string(),
        evidence_refs: vec![
            "git-evidence.json".to_string(),
            "command-evidence.json".to_string(),
        ],
        constraint_conflict: kind == RecoveryDecisionKind::Replan,
        same_strategy_viable: kind == RecoveryDecisionKind::Repair,
        worker_action,
        next_instruction: next_instruction.map(str::to_string),
        insufficient_authority: kind == RecoveryDecisionKind::BlockInsufficientAuthority,
        stagnation_detected: false,
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
    assert!(
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success()
    );
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

fn error_attempt(error: &str) -> ScriptedAttempt {
    ScriptedAttempt {
        match_worker: None,
        writes: Vec::new(),
        output: String::new(),
        error: Some(error.to_string()),
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
    assert!(state.active_lease_id.is_none());
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
    assert!(state.active_lease_id.is_none());
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
    assert!(
        state.steps[0].attempts[1]
            .repair_instruction
            .as_ref()
            .unwrap()
            .contains("Do not broaden")
    );
}

#[test]
fn acceptance_stderr_reaches_bounded_repair_instruction_and_fallback_task() {
    let fx = fixture();
    let stderr = (0..20)
        .map(|index| format!("error line {index}: mismatched types"))
        .collect::<Vec<_>>()
        .join("\n");
    let stdout = (0..8)
        .map(|index| format!("note line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let executor = HostExecutor::with_command_failure(&stdout, &stderr, 101);
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt("src/alpha.rs", "pub fn alpha() -> u8 { 1 }\n"),
            write_attempt("src/alpha.rs", "pub fn alpha() -> u8 { 1 }\n"),
            write_attempt("src/alpha.rs", "pub fn alpha() -> u8 { 1 }\n"),
        ],
    );
    let campaign = make_campaign(
        "acceptance-evidence",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let state = run_campaign(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);

    assert_eq!(state.steps[0].attempts.len(), 3);
    assert!(state.steps[0].attempts[0].repair_instruction.is_none());
    let repair_instruction = state.steps[0].attempts[1]
        .repair_instruction
        .as_deref()
        .unwrap();
    assert!(repair_instruction.contains("Failing command:"));
    assert!(repair_instruction.contains("Exit code: 101"));
    assert!(repair_instruction.contains("stderr:"));
    assert!(repair_instruction.contains("error line 0: mismatched types"));
    assert!(repair_instruction.contains("stdout:"));
    assert!(repair_instruction.contains("note line 0"));
    assert!(repair_instruction.contains("[truncated]"));
    assert!(!repair_instruction.contains("error line 19: mismatched types"));

    let seen_tasks = implementer.seen_tasks();
    assert!(seen_tasks[1].contains("Failing command: true"));
    assert!(seen_tasks[1].contains("Exit code: 101"));
    assert!(seen_tasks[2].contains("Failing command: true"));
    assert!(seen_tasks[2].contains("[truncated]"));
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
    assert!(
        executor
            .writes
            .lock()
            .unwrap()
            .contains(&"src/alpha.rs".to_string())
    );
    assert_eq!(
        implementer.seen_workers(),
        ["local-coder", "local-coder", "local-primary"]
    );
}

#[test]
fn reviewer_failure_fails_closed_and_persists_request_evidence() {
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
        "review-fail-closed",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&FAILING_REVIEWER);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::ReviewerTimeout)
    );
    let packet = fs::read_to_string(
        runner
            .attempt_dir(&campaign.campaign_id, "add-alpha", 1)
            .join("model-review.json"),
    )
    .unwrap();
    assert!(packet.contains("review endpoint timed out"));
    assert!(packet.contains("allowed_paths"));
    assert!(packet.contains("command_summary"));
    assert!(packet.contains("changed_paths"));
}

#[test]
fn transient_reviewer_timeout_retries_then_accepts_without_rerunning_implementation() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }
",
        )],
    );
    let reviewer = ScriptedReviewer::new(vec![
        Err("coordinator review request failed or timed out: timeout: global".to_string()),
        Ok(ModelReviewResult {
            disposition: CoordinatorReviewDisposition::Accepted,
            classification: None,
            rationale: "review accepted after retry".to_string(),
            prompt: "prompt".to_string(),
            raw_output: r#"{"disposition":"accepted","classification":null,"rationale":"ok"}"#
                .to_string(),
            used_host_shell: false,
        }),
    ]);
    let campaign = make_campaign(
        "review-retry-accept",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Completed);
    assert_eq!(state.steps[0].attempts.len(), 1);
    assert_eq!(implementer.seen_tasks().len(), 1);
    assert_eq!(reviewer.calls(), 2);
}

#[test]
fn repeated_reviewer_timeouts_fail_closed_without_worker_timeout_classification() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }
",
        )],
    );
    let reviewer = ScriptedReviewer::new(vec![
        Err("coordinator review request failed or timed out: timeout: global".to_string()),
        Err("coordinator review request failed or timed out: timeout: global".to_string()),
        Err("coordinator review request failed or timed out: timeout: global".to_string()),
    ]);
    let campaign = make_campaign(
        "review-retry-fail",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::ReviewerTimeout)
    );
    assert_ne!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::WorkerTimeout)
    );
    assert_eq!(implementer.seen_tasks().len(), 1);
    assert_eq!(reviewer.calls(), 3);
}

#[test]
fn finalization_timeout_with_valid_change_runs_semantic_review_and_commits_once() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let reviewer = ScriptedReviewer::new(vec![Ok(ModelReviewResult {
        disposition: CoordinatorReviewDisposition::Accepted,
        classification: None,
        rationale: "review accepted after worker finalization timeout".to_string(),
        prompt: "prompt".to_string(),
        raw_output: r#"{"disposition":"accepted","classification":null,"rationale":"ok"}"#
            .to_string(),
        used_host_shell: false,
    })]);
    let mut attempt = write_attempt("src/fallback.rs", "pub fn fallback() -> i32 { 2 }\n");
    attempt.output = String::new();
    attempt.error = Some("model request failed or timed out: timeout: global".to_string());
    let implementer = ScriptedChangeImplementer::new(&executor, vec![attempt]);
    let campaign = make_campaign(
        "finalization-timeout-accepted",
        &fx.sha,
        vec![sample_step("fallback", "src/fallback.rs", "Add fallback.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Completed);
    assert_eq!(state.steps[0].attempts.len(), 1);
    assert_eq!(state.steps[0].attempts[0].commit_sha.is_some(), true);
    assert_eq!(state.steps[0].attempts[0].classification, None);
    assert_eq!(implementer.seen_tasks().len(), 1);
    assert_eq!(reviewer.calls(), 1);
    let transcript = fs::read_to_string(
        runner
            .attempt_dir(&campaign.campaign_id, "fallback", 1)
            .join("worker-transcript.json"),
    )
    .unwrap();
    assert!(transcript.contains("timeout: global"));
}

#[test]
fn finalization_timeout_with_acceptance_failure_is_rejected_without_review() {
    let fx = fixture();
    let stderr = "compile error";
    let stdout = "";
    let executor = HostExecutor::with_command_failure(stdout, stderr, 101);
    let mut attempt = write_attempt("src/fallback.rs", "pub fn fallback() -> i32 { 2 }\n");
    attempt.output = String::new();
    attempt.error = Some("model request failed or timed out: timeout: global".to_string());
    let implementer = ScriptedChangeImplementer::new(&executor, vec![attempt]);
    let reviewer = ScriptedReviewer::new(vec![]);
    let campaign = make_campaign(
        "finalization-timeout-acceptance-fail",
        &fx.sha,
        vec![sample_step("fallback", "src/fallback.rs", "Add fallback.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::AcceptanceFailed)
    );
    assert_eq!(reviewer.calls(), 0);
}

#[test]
fn finalization_timeout_with_no_meaningful_diff_is_rejected_without_review() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![error_attempt(
            "model request failed or timed out: timeout: global",
        )],
    );
    let reviewer = ScriptedReviewer::new(vec![]);
    let campaign = make_campaign(
        "finalization-timeout-no-diff",
        &fx.sha,
        vec![sample_step("fallback", "src/fallback.rs", "Add fallback.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::WorkerTimeout)
    );
    assert_eq!(reviewer.calls(), 0);
}

#[test]
fn finalization_timeout_with_out_of_policy_diff_fails_closed_without_review() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let mut attempt = write_attempt("src/forbidden.rs", "pub fn forbidden() -> i32 { 2 }\n");
    attempt.output = String::new();
    attempt.error = Some("model request failed or timed out: timeout: global".to_string());
    let implementer = ScriptedChangeImplementer::new(&executor, vec![attempt]);
    let reviewer = ScriptedReviewer::new(vec![]);
    let mut step = sample_step("fallback", "src/fallback.rs", "Add fallback.");
    step.allowed_paths = vec!["src/fallback.rs".to_string()];
    let campaign = make_campaign(
        "finalization-timeout-path-policy",
        &fx.sha,
        vec![step],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::PathPolicyFailed)
    );
    assert_eq!(reviewer.calls(), 0);
}

#[test]
fn finalization_timeout_followed_by_reviewer_rejection_does_not_commit() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let mut attempt = write_attempt("src/fallback.rs", "pub fn fallback() -> i32 { 2 }\n");
    attempt.output = String::new();
    attempt.error = Some("model request failed or timed out: timeout: global".to_string());
    let implementer = ScriptedChangeImplementer::new(&executor, vec![attempt]);
    let reviewer = ScriptedReviewer::new(vec![Ok(ModelReviewResult {
        disposition: CoordinatorReviewDisposition::RejectedRetryable,
        classification: Some(FailureClassification::InadequateImplementation),
        rationale: "semantic rejection".to_string(),
        prompt: "prompt".to_string(),
        raw_output: r#"{"disposition":"rejected_retryable","classification":"inadequate_implementation","rationale":"semantic rejection"}"#.to_string(),
        used_host_shell: false,
    })]);
    let campaign = make_campaign(
        "finalization-timeout-review-reject",
        &fx.sha,
        vec![sample_step("fallback", "src/fallback.rs", "Add fallback.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Blocked);
    assert!(state.steps[0].attempts[0].commit_sha.is_none());
    assert_eq!(reviewer.calls(), 1);
}

#[test]
fn finalization_timeout_reviewer_transport_retry_does_not_rerun_implementation() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let mut attempt = write_attempt("src/fallback.rs", "pub fn fallback() -> i32 { 2 }\n");
    attempt.output = String::new();
    attempt.error = Some("model request failed or timed out: timeout: global".to_string());
    let implementer = ScriptedChangeImplementer::new(&executor, vec![attempt]);
    let reviewer = ScriptedReviewer::new(vec![
        Err("coordinator review request failed or timed out: timeout: global".to_string()),
        Ok(ModelReviewResult {
            disposition: CoordinatorReviewDisposition::Accepted,
            classification: None,
            rationale: "review accepted after retry".to_string(),
            prompt: "prompt".to_string(),
            raw_output: r#"{"disposition":"accepted","classification":null,"rationale":"ok"}"#
                .to_string(),
            used_host_shell: false,
        }),
    ]);
    let campaign = make_campaign(
        "finalization-timeout-review-retry",
        &fx.sha,
        vec![sample_step("fallback", "src/fallback.rs", "Add fallback.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Completed);
    assert_eq!(implementer.seen_tasks().len(), 1);
    assert_eq!(reviewer.calls(), 2);
}

#[test]
fn attempt_repair_instruction_preserves_launch_causality_after_later_failure() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            empty_attempt("COMPLETE"),
            error_attempt("model request failed or timed out: timeout: global"),
        ],
    );
    let campaign = make_campaign(
        "attempt-launch-causality",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 1,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    let attempt_one_instruction = state.steps[0].attempts[0]
        .next_repair_instruction
        .as_deref()
        .unwrap()
        .to_string();
    let attempt_two = &state.steps[0].attempts[1];
    assert_eq!(
        attempt_two.classification,
        Some(FailureClassification::WorkerTimeout)
    );
    assert_eq!(
        attempt_two.repair_instruction.as_deref(),
        Some(attempt_one_instruction.as_str())
    );
    let transcript = fs::read_to_string(
        runner
            .attempt_dir(&campaign.campaign_id, "add-alpha", 2)
            .join("worker-transcript.json"),
    )
    .unwrap();
    assert!(transcript.contains("implementation produced no source diff"));
    assert!(transcript.contains("timeout: global"));
    let packet = fs::read_to_string(
        runner
            .attempt_dir(&campaign.campaign_id, "add-alpha", 2)
            .join("review-packet.json"),
    )
    .unwrap();
    assert!(packet.contains("implementation produced no source diff"));
    assert!(packet.contains("next_repair_instruction"));
}

#[test]
fn semantic_rejection_is_not_retried() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/domain/mod.rs",
            "pub struct DomainId;
",
        )],
    );
    let reviewer = ScriptedReviewer::new(vec![Ok(ModelReviewResult {
        disposition: CoordinatorReviewDisposition::RejectedRetryable,
        classification: Some(FailureClassification::InadequateImplementation),
        rationale: "missing requested file".to_string(),
        prompt: "prompt".to_string(),
        raw_output: r#"{"disposition":"rejected_retryable","classification":"inadequate_implementation","rationale":"missing requested file"}"#.to_string(),
        used_host_shell: false,
    })]);
    let mut step = sample_step("domain", "src/domain/mod.rs", "Add domain identifiers.");
    step.required_changed_paths = vec!["src/domain/".to_string()];
    step.acceptance.required_artifacts.clear();
    let campaign = make_campaign(
        "review-reject-no-retry",
        &fx.sha,
        vec![step],
        WorkerPolicy {
            primary_attempts: 1,
            repair_attempts: 0,
            fallback_attempts: 0,
            ..default_policy()
        },
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(reviewer.calls(), 1);
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
        sleeper: &NOOP_SLEEPER,
        worker_recovery_max_wait_seconds: 900,
        worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
        worker_recovery_max_attempts: 11,
        state_root: fx.root.clone(),
        container_tracker: None,
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
    assert!(
        blocked
            .error_message
            .unwrap()
            .contains("detached runner setup failed")
    );
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
    assert!(paused.active_lease_id.is_none());
    let resumed = runner.resume("pause").unwrap();
    assert_eq!(resumed.state, CampaignState::Completed);
    assert!(resumed.active_lease_id.is_none());
}

#[test]
fn resume_does_not_persist_running_if_lease_acquire_fails() {
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
        "resume-lease",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    runner.pause("resume-lease").unwrap();
    runner.run("resume-lease").unwrap();
    let paused = runner.load_state("resume-lease").unwrap().unwrap();
    assert_eq!(paused.state, CampaignState::Paused);
    assert!(paused.pause_requested);
    let lease_dir = runner.campaign_dir("resume-lease");
    fs::create_dir_all(&lease_dir).unwrap();
    fs::write(
        lease_dir.join("lease.json"),
        r#"{
  "campaign_id": "resume-lease",
  "repository_id": "fixture",
  "pid": 1,
  "acquired_at": "1000",
  "heartbeat": "1000",
  "heartbeat_seconds": 10,
  "action_timeout_seconds": 60
}
"#,
    )
    .unwrap();
    let error = runner.resume("resume-lease").unwrap_err();
    assert!(error.contains("live pid"), "{error}");
    let after = runner.load_state("resume-lease").unwrap().unwrap();
    assert_eq!(after.state, CampaignState::Paused);
    assert!(after.pause_requested);
    assert!(after.active_lease_id.is_none());
}

#[test]
fn lease_action_timeout_includes_revision_steps() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let campaign = make_campaign(
        "lease-timeout",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    assert_eq!(runner.lease_action_timeout(&campaign).unwrap(), 60);
    let mut state = runner.load_state("lease-timeout").unwrap().unwrap();
    state.state = CampaignState::Paused;
    runner.save_state(&state).unwrap();
    let mut long_step = sample_step("add-beta", "src/beta.rs", "Add beta.");
    long_step.limits.timeout_seconds = 900;
    runner
        .revise(
            "lease-timeout",
            CampaignRevisionDocument {
                instruction: "Add a longer bounded step.".to_string(),
                steps: vec![long_step],
            },
        )
        .unwrap();
    assert_eq!(runner.lease_action_timeout(&campaign).unwrap(), 900);
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
fn stale_runner_save_cannot_erase_operator_cancel_pause_or_revision() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let campaign = make_campaign(
        "state-merge",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    let stale = runner.start(&campaign).unwrap();

    runner.pause("state-merge").unwrap();
    let mut paused = runner.load_state("state-merge").unwrap().unwrap();
    paused.state = CampaignState::Paused;
    runner.save_state(&paused).unwrap();
    runner
        .revise(
            "state-merge",
            CampaignRevisionDocument {
                instruction: "append beta".to_string(),
                steps: vec![sample_step("add-beta", "src/beta.rs", "Add beta.")],
            },
        )
        .unwrap();
    runner.cancel("state-merge", Some("operator stop")).unwrap();

    runner.save_state(&stale).unwrap();
    let saved = runner.load_state("state-merge").unwrap().unwrap();
    assert!(saved.cancel_requested);
    assert_eq!(saved.state, CampaignState::Cancelled);
    assert_eq!(saved.revisions.len(), 1);
    assert!(saved.steps.iter().any(|step| step.step_id == "add-beta"));
}

#[test]
fn stale_runner_save_preserves_pause_until_intentional_resume() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let campaign = make_campaign(
        "pause-merge",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    let stale = runner.start(&campaign).unwrap();
    runner.pause("pause-merge").unwrap();
    runner.save_state(&stale).unwrap();
    assert!(
        runner
            .load_state("pause-merge")
            .unwrap()
            .unwrap()
            .pause_requested
    );
}

#[test]
fn background_state_heartbeat_cannot_resurrect_paused_campaign() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let campaign = make_campaign(
        "paused-heartbeat",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    runner.pause("paused-heartbeat").unwrap();
    runner.run("paused-heartbeat").unwrap();

    let before = runner.load_state("paused-heartbeat").unwrap().unwrap();
    assert_eq!(before.state, CampaignState::Paused);

    let error = runner
        .test_background_state_heartbeat(
            "paused-heartbeat",
            Some("add-alpha"),
            Some("local-coder"),
            "model_request",
        )
        .unwrap_err();
    assert!(error.contains("no longer running"), "{error}");

    let after = runner.load_state("paused-heartbeat").unwrap().unwrap();
    assert_eq!(after, before);
}

#[test]
fn background_state_heartbeat_cannot_resurrect_cancelled_campaign() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let campaign = make_campaign(
        "cancelled-heartbeat",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    runner.cancel("cancelled-heartbeat", Some("stop")).unwrap();
    runner.run("cancelled-heartbeat").unwrap();

    let before = runner.load_state("cancelled-heartbeat").unwrap().unwrap();
    assert_eq!(before.state, CampaignState::Cancelled);

    let error = runner
        .test_background_state_heartbeat(
            "cancelled-heartbeat",
            Some("add-alpha"),
            Some("local-coder"),
            "model_request",
        )
        .unwrap_err();
    assert!(error.contains("no longer running"), "{error}");

    let after = runner.load_state("cancelled-heartbeat").unwrap().unwrap();
    assert_eq!(after, before);
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
    assert!(state.active_lease_id.is_none());
    assert!(state.steps[0].accepted_commit.is_none());
    assert!(runner.campaign_dir("cancel").join("campaign.json").exists());
}

#[test]
fn supervisor_resumes_running_campaigns() {
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
        "supervise-running",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(5_000),
    };
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    let supervisor = CampaignSupervisor::new(CampaignSupervisorDependencies {
        runner: &runner,
        clock: &clock,
        state_root: fx.root.clone(),
        workspace_root: fx.workspaces.clone(),
        operations: OperationsConfig {
            schema_version: "rack-ai/operations/v1".to_string(),
            supervisor: SupervisorConfig {
                scan_interval_seconds: 10,
                resume_running_campaigns: true,
                podman_command: "true".to_string(),
                worker_recovery_max_wait_seconds: 900,
                worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
                worker_recovery_max_attempts: 11,
            },
            retention: RetentionConfig {
                max_terminal_campaign_age_seconds: 3_600,
                retain_terminal_campaigns: 1,
                max_auxiliary_artifact_age_seconds: 3_600,
                retain_auxiliary_artifacts: 1,
            },
        },
    })
    .unwrap();

    let report = supervisor.run_once().unwrap();
    assert_eq!(report.resumed_campaigns, 1);
    assert_eq!(report.actions[0].action, "resume");
    assert_eq!(
        report.actions[0].outcome_state,
        Some(CampaignState::Completed)
    );
    assert_eq!(
        runner
            .load_state("supervise-running")
            .unwrap()
            .unwrap()
            .state,
        CampaignState::Completed
    );
}

#[test]
fn supervisor_prunes_old_terminal_campaigns_beyond_retention() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let old_campaign = make_campaign(
        "old-terminal",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let new_campaign = make_campaign(
        "new-terminal",
        &fx.sha,
        vec![sample_step("add-beta", "src/beta.rs", "Add beta.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &old_campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&old_campaign).unwrap();
    runner.start(&new_campaign).unwrap();
    let mut old_state = runner.load_state("old-terminal").unwrap().unwrap();
    old_state.state = CampaignState::Completed;
    old_state.end_time = Some("100".to_string());
    runner.save_state(&old_state).unwrap();
    let mut new_state = runner.load_state("new-terminal").unwrap().unwrap();
    new_state.state = CampaignState::Completed;
    new_state.end_time = Some("4900".to_string());
    runner.save_state(&new_state).unwrap();
    let clock = TestClock {
        now: Cell::new(5_000),
    };
    let supervisor = CampaignSupervisor::new(CampaignSupervisorDependencies {
        runner: &runner,
        clock: &clock,
        state_root: fx.root.clone(),
        workspace_root: fx.workspaces.clone(),
        operations: OperationsConfig {
            schema_version: "rack-ai/operations/v1".to_string(),
            supervisor: SupervisorConfig {
                scan_interval_seconds: 10,
                resume_running_campaigns: true,
                podman_command: "true".to_string(),
                worker_recovery_max_wait_seconds: 900,
                worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
                worker_recovery_max_attempts: 11,
            },
            retention: RetentionConfig {
                max_terminal_campaign_age_seconds: 3_600,
                retain_terminal_campaigns: 1,
                max_auxiliary_artifact_age_seconds: 3_600,
                retain_auxiliary_artifacts: 1,
            },
        },
    })
    .unwrap();

    let report = supervisor.run_once().unwrap();
    assert_eq!(report.cleanup.len(), 1);
    assert_eq!(report.cleanup[0].campaign_id, "old-terminal");
    assert!(runner.load_state("old-terminal").unwrap().is_none());
    assert!(
        !fx.workspaces
            .join("campaign-old-terminal")
            .join("repo")
            .exists()
    );
    assert!(runner.load_state("new-terminal").unwrap().is_some());
}

#[test]
fn supervisor_removes_stale_orphan_repository_leases() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let campaign = make_campaign(
        "lease-anchor",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    let lease_dir = fx.root.join("state/campaigns/.repository-leases");
    fs::create_dir_all(&lease_dir).unwrap();
    fs::write(
        lease_dir.join("fixture.json"),
        r#"{
  "campaign_id": "missing-campaign",
  "repository_id": "fixture",
  "pid": 1,
  "acquired_at": "1",
  "heartbeat": "1",
  "heartbeat_seconds": 10,
  "action_timeout_seconds": 60
}
"#,
    )
    .unwrap();
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let supervisor = CampaignSupervisor::new(CampaignSupervisorDependencies {
        runner: &runner,
        clock: &clock,
        state_root: fx.root.clone(),
        workspace_root: fx.workspaces.clone(),
        operations: OperationsConfig {
            schema_version: "rack-ai/operations/v1".to_string(),
            supervisor: SupervisorConfig {
                scan_interval_seconds: 10,
                resume_running_campaigns: true,
                podman_command: "true".to_string(),
                worker_recovery_max_wait_seconds: 900,
                worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
                worker_recovery_max_attempts: 11,
            },
            retention: RetentionConfig {
                max_terminal_campaign_age_seconds: 3_600,
                retain_terminal_campaigns: 1,
                max_auxiliary_artifact_age_seconds: 3_600,
                retain_auxiliary_artifacts: 1,
            },
        },
    })
    .unwrap();

    let report = supervisor.run_once().unwrap();
    assert_eq!(report.cleanup.len(), 1);
    assert_eq!(report.cleanup[0].action, "remove_orphan_repository_lease");
    assert!(!lease_dir.join("fixture.json").exists());
}

#[test]
fn supervisor_cleans_stale_campaign_container_before_resume() {
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
        "supervise-container",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(5_000),
    };
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    let mut state = runner.load_state("supervise-container").unwrap().unwrap();
    state.active_container_id = Some("ghost-container".to_string());
    runner.save_state(&state).unwrap();
    fs::write(
        runner
            .campaign_dir("supervise-container")
            .join("active-container.json"),
        r#"{
  "campaign_id": "supervise-container",
  "step_id": "add-alpha",
  "action": "model_request",
  "container_id": "ghost-container",
  "recorded_at": "1000"
}
"#,
    )
    .unwrap();
    let supervisor = CampaignSupervisor::new(CampaignSupervisorDependencies {
        runner: &runner,
        clock: &clock,
        state_root: fx.root.clone(),
        workspace_root: fx.workspaces.clone(),
        operations: OperationsConfig {
            schema_version: "rack-ai/operations/v1".to_string(),
            supervisor: SupervisorConfig {
                scan_interval_seconds: 10,
                resume_running_campaigns: true,
                podman_command: "true".to_string(),
                worker_recovery_max_wait_seconds: 900,
                worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
                worker_recovery_max_attempts: 11,
            },
            retention: RetentionConfig {
                max_terminal_campaign_age_seconds: 3_600,
                retain_terminal_campaigns: 1,
                max_auxiliary_artifact_age_seconds: 3_600,
                retain_auxiliary_artifacts: 1,
            },
        },
    })
    .unwrap();

    let report = supervisor.run_once().unwrap();
    assert!(
        report
            .cleanup
            .iter()
            .any(|item| item.action == "cleanup_stale_campaign_container")
    );
    let after = runner.load_state("supervise-container").unwrap().unwrap();
    assert_eq!(after.state, CampaignState::Completed);
    assert!(after.active_container_id.is_none());
    assert!(
        !runner
            .campaign_dir("supervise-container")
            .join("active-container.json")
            .exists()
    );
}

#[test]
fn supervisor_prunes_auxiliary_artifacts_beyond_retention() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, Vec::new());
    let campaign = make_campaign(
        "auxiliary-retention",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    let logs_dir = fx.root.join("logs/runs");
    let changes_dir = fx.root.join("state/changes");
    fs::create_dir_all(&logs_dir).unwrap();
    fs::create_dir_all(&changes_dir).unwrap();
    let old_log = logs_dir.join("old.json");
    let new_log = logs_dir.join("new.json");
    fs::write(&old_log, "old").unwrap();
    fs::write(&new_log, "new").unwrap();
    let old_change = changes_dir.join("old-change");
    let new_change = changes_dir.join("new-change");
    fs::create_dir_all(&old_change).unwrap();
    fs::create_dir_all(&new_change).unwrap();
    fs::write(
        old_change.join("review-packet.json"),
        "{}
",
    )
    .unwrap();
    fs::write(
        new_change.join("review-packet.json"),
        "{}
",
    )
    .unwrap();
    let clock = TestClock {
        now: Cell::new(10_000_000_000),
    };
    let supervisor = CampaignSupervisor::new(CampaignSupervisorDependencies {
        runner: &runner,
        clock: &clock,
        state_root: fx.root.clone(),
        workspace_root: fx.workspaces.clone(),
        operations: OperationsConfig {
            schema_version: "rack-ai/operations/v1".to_string(),
            supervisor: SupervisorConfig {
                scan_interval_seconds: 10,
                resume_running_campaigns: true,
                podman_command: "true".to_string(),
                worker_recovery_max_wait_seconds: 900,
                worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
                worker_recovery_max_attempts: 11,
            },
            retention: RetentionConfig {
                max_terminal_campaign_age_seconds: 3_600,
                retain_terminal_campaigns: 1,
                max_auxiliary_artifact_age_seconds: 3_600,
                retain_auxiliary_artifacts: 1,
            },
        },
    })
    .unwrap();

    let report = supervisor.run_once().unwrap();
    assert!(
        report
            .cleanup
            .iter()
            .filter(|item| item.action == "prune_auxiliary_artifact")
            .count()
            >= 2
    );
    assert!(!old_log.exists());
    assert!(new_log.exists());
    assert!(!old_change.exists());
    assert!(new_change.exists());
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
        sleeper: &NOOP_SLEEPER,
        worker_recovery_max_wait_seconds: 900,
        worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
        worker_recovery_max_attempts: 11,
        state_root: fx.root.clone(),
        container_tracker: None,
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
fn recovery_retries_worker_health_until_endpoint_recovers_without_consuming_attempts() {
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
        "recovery-waits",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = RecordingSleeper::new(&clock);
    let health = FlakyWorkerHealth::new(1, vec!["io: Peer disconnected", "io: Peer disconnected"]);
    let runner = make_runner_with_support(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &health,
        &clock,
        &sleeper,
    );
    runner.start(&campaign).unwrap();

    let completed = runner.run("recovery-waits").unwrap();

    assert_eq!(completed.state, CampaignState::Completed);
    assert_eq!(completed.steps[0].attempts.len(), 1);
    assert_eq!(implementer.seen_tasks().len(), 1);
    assert_eq!(sleeper.sleeps(), vec![5, 10]);
    let events = fs::read_to_string(runner.events_path("recovery-waits")).unwrap();
    assert!(events.contains("dependency_recovery_waiting"));
    assert!(events.contains("dependency_recovery_ready"));
}

#[test]
fn recovery_blocks_after_bounded_worker_health_wait_without_consuming_attempts() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, vec![]);
    let campaign = make_campaign(
        "recovery-blocks",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = RecordingSleeper::new(&clock);
    let health = FlakyWorkerHealth::new(
        1,
        vec![
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
        ],
    );
    let runner = make_runner_with_support(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &health,
        &clock,
        &sleeper,
    );
    runner.start(&campaign).unwrap();

    let blocked = runner.run("recovery-blocks").unwrap();

    assert_eq!(blocked.state, CampaignState::Blocked);
    assert_eq!(blocked.blocked_reason.as_deref(), Some("model_unavailable"));
    assert!(
        blocked
            .error_message
            .as_deref()
            .unwrap()
            .contains("bounded recovery wait")
    );
    assert!(blocked.steps[0].attempts.is_empty());
    assert_eq!(implementer.seen_tasks().len(), 0);
    assert_eq!(
        sleeper.sleeps(),
        vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120]
    );
}

#[test]
fn recovery_wait_honors_pause_before_any_attempt_runs() {
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
        "recovery-pause",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = PauseRequestSleeper::new(&clock, fx.root.clone(), "recovery-pause");
    let health = FlakyWorkerHealth::new(1, vec!["io: Peer disconnected", "io: Peer disconnected"]);
    let runner = make_runner_with_support(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &health,
        &clock,
        &sleeper,
    );
    runner.start(&campaign).unwrap();

    let paused = runner.run("recovery-pause").unwrap();

    assert_eq!(paused.state, CampaignState::Paused);
    assert!(paused.pause_requested);
    assert!(paused.steps[0].attempts.is_empty());
    assert_eq!(implementer.seen_tasks().len(), 0);
}

#[test]
fn resume_preflight_does_not_require_other_worker_after_current_worker_recovers() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }
",
        )],
    );
    let campaign = make_campaign(
        "resume-current-worker-only",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let start_runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    start_runner.start(&campaign).unwrap();
    let mut state = start_runner
        .load_state(&campaign.campaign_id)
        .unwrap()
        .unwrap();
    state.state = CampaignState::Running;
    state.current_step_id = Some("add-alpha".to_string());
    state.current_attempt = 1;
    state.current_worker = Some("local-coder".to_string());
    state.current_action = Some("model_request".to_string());
    start_runner.save_state(&state).unwrap();

    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = RecordingSleeper::new(&clock);
    let health = ResumeCurrentWorkerHealthyOtherUnavailable::new(5);
    let resume_runner = make_runner_with_support(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &health,
        &clock,
        &sleeper,
    );

    let completed = resume_runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(completed.state, CampaignState::Completed);
    assert_eq!(completed.current_attempt, 1);
    assert_eq!(completed.steps[0].attempts.len(), 1);
    assert_eq!(implementer.seen_workers(), vec!["local-coder".to_string()]);
    assert_eq!(sleeper.sleeps(), vec![5, 10, 15, 20, 30]);
    let events = fs::read_to_string(resume_runner.events_path(&campaign.campaign_id)).unwrap();
    let recovered = events.find("campaign_recovered").unwrap();
    let started = events.rfind("model_request_started").unwrap();
    assert!(started > recovered);
}

#[test]
fn invalid_static_preflight_condition_still_fails_closed() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }
",
        )],
    );
    let campaign = make_campaign(
        "invalid-static-preflight",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    let mut broken = campaign.clone();
    broken.worker_policy.fallback = "missing-worker".to_string();
    fs::write(
        runner
            .campaign_dir(&campaign.campaign_id)
            .join("campaign.json"),
        serde_json::to_string_pretty(&broken).unwrap()
            + "
",
    )
    .unwrap();

    let blocked = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(blocked.state, CampaignState::Blocked);
    assert_eq!(blocked.blocked_reason.as_deref(), Some("continuity_failed"));
    assert_eq!(implementer.seen_tasks().len(), 0);
}

#[test]
fn fallback_worker_unavailability_recovers_when_fallback_is_selected() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            error_attempt("connection refused"),
            write_attempt(
                "src/alpha.rs",
                "pub fn alpha() -> u8 { 1 }
",
            ),
        ],
    );
    let mut policy = default_policy();
    policy.primary_attempts = 0;
    policy.repair_attempts = 0;
    policy.fallback_attempts = 1;
    let campaign = make_campaign(
        "fallback-recovery",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        policy,
    );
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = RecordingSleeper::new(&clock);
    let runner = make_runner_with_support(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &Healthy,
        &clock,
        &sleeper,
    );
    runner.start(&campaign).unwrap();

    let completed = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(completed.state, CampaignState::Completed);
    assert_eq!(completed.current_attempt, 1);
    assert_eq!(completed.steps[0].attempts.len(), 1);
    assert_eq!(
        implementer.seen_workers(),
        vec!["local-primary".to_string(), "local-primary".to_string()]
    );
    assert_eq!(sleeper.sleeps(), vec![5]);
}

#[test]
fn transient_model_request_transport_failure_reenters_bounded_recovery_and_continues_same_attempt()
{
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            error_attempt("io: Peer disconnected"),
            write_attempt(
                "src/alpha.rs",
                "pub fn alpha() -> u8 { 1 }
",
            ),
        ],
    );
    let campaign = make_campaign(
        "transport-retry-success",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = RecordingSleeper::new(&clock);
    let runner = make_runner_with_support(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &Healthy,
        &clock,
        &sleeper,
    );
    runner.start(&campaign).unwrap();

    let completed = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(completed.state, CampaignState::Completed);
    assert_eq!(completed.current_attempt, 1);
    assert_eq!(completed.steps[0].attempts.len(), 1);
    assert_eq!(completed.steps[0].attempts[0].attempt, 1);
    assert_eq!(implementer.seen_tasks().len(), 2);
    assert_eq!(sleeper.sleeps(), vec![5]);
    let events = fs::read_to_string(runner.events_path(&campaign.campaign_id)).unwrap();
    assert!(events.contains("dependency_recovery_waiting"));
    assert!(events.contains("dependency_recovery_ready"));
}

#[test]
fn persistent_transient_model_request_failure_eventually_blocks_without_consuming_attempt() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            error_attempt("connection refused"),
            error_attempt("connection refused"),
            error_attempt("connection refused"),
        ],
    );
    let campaign = make_campaign(
        "transport-retry-blocks",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = RecordingSleeper::new(&clock);
    let runner = CampaignRunner::new(CampaignRunnerDependencies {
        registry: Box::leak(Box::new(TestRegistry::new(
            fx.repo.clone(),
            fx.workspaces.clone(),
        ))),
        command_policy: &ALLOW_ALL,
        git: &PROCESS_GIT,
        implementer: &implementer,
        executor: &executor,
        workers: &WORKERS,
        health: &Healthy,
        clock: &clock,
        sleeper: &sleeper,
        worker_recovery_max_wait_seconds: 10,
        worker_recovery_retry_delays_seconds: vec![1, 1],
        worker_recovery_max_attempts: 3,
        state_root: fx.root.clone(),
        container_tracker: None,
    });
    runner.start(&campaign).unwrap();

    let blocked = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(blocked.state, CampaignState::Blocked);
    assert_eq!(blocked.blocked_reason.as_deref(), Some("model_unavailable"));
    assert_eq!(blocked.current_attempt, 1);
    assert!(blocked.steps[0].attempts.is_empty());
    assert_eq!(implementer.seen_tasks().len(), 3);
    assert_eq!(sleeper.sleeps(), vec![1, 1]);
}

#[test]
fn non_transport_worker_failure_does_not_enter_dependency_recovery() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            error_attempt("worker produced invalid code output"),
            error_attempt("worker produced invalid code output"),
            error_attempt("worker produced invalid code output"),
        ],
    );
    let campaign = make_campaign(
        "transport-non-retry",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = RecordingSleeper::new(&clock);
    let runner = make_runner_with_support(
        &fx,
        &campaign,
        &implementer,
        &executor,
        &Healthy,
        &clock,
        &sleeper,
    );
    runner.start(&campaign).unwrap();

    let blocked = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(blocked.state, CampaignState::Blocked);
    assert_eq!(implementer.seen_tasks().len(), 3);
    assert!(sleeper.sleeps().is_empty());
    let events = fs::read_to_string(runner.events_path(&campaign.campaign_id)).unwrap();
    assert!(!events.contains("dependency_recovery_waiting"));
}

#[test]
fn recovery_resets_interrupted_campaign_owned_dirty_worktree_and_continues() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }
",
        )],
    );
    let campaign = make_campaign(
        "reboot-owned-dirty",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    let started = runner.start(&campaign).unwrap();
    let mut state = runner.load_state(&campaign.campaign_id).unwrap().unwrap();
    state.current_step_id = Some("add-alpha".to_string());
    state.current_attempt = 1;
    state.current_worker = Some("local-coder".to_string());
    state.current_action = Some("model_request".to_string());
    runner.save_state(&state).unwrap();
    fs::write(
        Path::new(&started.worktree_path).join("src/alpha.rs"),
        "partial
",
    )
    .unwrap();
    fs::write(
        Path::new(&started.worktree_path).join("src/reboot.rs"),
        "partial
",
    )
    .unwrap();
    fs::write(
        fx.repo.join("src/source_only.rs"),
        "leave me alone
",
    )
    .unwrap();

    let completed = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(completed.state, CampaignState::Completed);
    assert_eq!(completed.steps[0].attempts.len(), 1);
    assert_eq!(implementer.seen_tasks().len(), 1);
    assert!(
        !Path::new(&completed.worktree_path)
            .join("src/reboot.rs")
            .exists()
    );
    let evidence = fs::read_to_string(
        runner
            .campaign_dir(&campaign.campaign_id)
            .join("recovery-reset-attempt-1.json"),
    )
    .unwrap();
    assert!(evidence.contains("src/alpha.rs"));
    assert!(evidence.contains("src/reboot.rs"));
    assert!(evidence.contains("model_request"));
    assert_eq!(
        fs::read_to_string(fx.repo.join("src/source_only.rs")).unwrap(),
        "leave me alone
"
    );
}

#[test]
fn recovery_blocks_on_unknown_dirty_worktree_changes() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }
",
        )],
    );
    let campaign = make_campaign(
        "reboot-unknown-dirty",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    let started = runner.start(&campaign).unwrap();
    let mut state = runner.load_state(&campaign.campaign_id).unwrap().unwrap();
    state.current_step_id = Some("add-alpha".to_string());
    state.current_attempt = 1;
    state.current_worker = Some("local-coder".to_string());
    state.current_action = Some("model_request".to_string());
    runner.save_state(&state).unwrap();
    fs::write(
        Path::new(&started.worktree_path).join("README.md"),
        "partial
",
    )
    .unwrap();

    let blocked = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(blocked.state, CampaignState::Blocked);
    assert_eq!(blocked.blocked_reason.as_deref(), Some("continuity_failed"));
    assert!(
        !runner
            .campaign_dir(&campaign.campaign_id)
            .join("recovery-reset-attempt-1.json")
            .exists()
    );
}

#[test]
fn recovery_never_discards_dirty_worktree_after_accepted_commit() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }
",
        )],
    );
    let campaign = make_campaign(
        "reboot-accepted-dirty",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000);
    runner.start(&campaign).unwrap();
    let completed = runner.run(&campaign.campaign_id).unwrap();
    let mut state = runner.load_state(&campaign.campaign_id).unwrap().unwrap();
    state.state = CampaignState::Running;
    state.current_step_id = Some("add-alpha".to_string());
    state.current_attempt = 2;
    state.current_worker = Some("local-coder".to_string());
    state.current_action = Some("model_request".to_string());
    state.current_head_sha = completed.current_head_sha.clone();
    runner.save_state(&state).unwrap();
    fs::write(
        Path::new(&completed.worktree_path).join("src/alpha.rs"),
        "dirty again
",
    )
    .unwrap();

    let blocked = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(blocked.state, CampaignState::Blocked);
    assert_eq!(blocked.blocked_reason.as_deref(), Some("continuity_failed"));
    assert_eq!(
        fs::read_to_string(Path::new(&completed.worktree_path).join("src/alpha.rs")).unwrap(),
        "dirty again
"
    );
}

#[test]
fn recovery_uses_configured_worker_recovery_policy() {
    let fx = fixture();
    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![write_attempt(
            "src/alpha.rs",
            "pub fn alpha() -> u8 { 1 }
",
        )],
    );
    let campaign = make_campaign(
        "recovery-custom-policy",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let clock = TestClock {
        now: Cell::new(1_000),
    };
    let sleeper = RecordingSleeper::new(&clock);
    let health = FlakyWorkerHealth::new(
        1,
        vec![
            "io: Peer disconnected",
            "io: Peer disconnected",
            "io: Peer disconnected",
        ],
    );
    let runner = CampaignRunner::new(CampaignRunnerDependencies {
        registry: Box::leak(Box::new(TestRegistry::new(
            fx.repo.clone(),
            fx.workspaces.clone(),
        ))),
        command_policy: &ALLOW_ALL,
        git: &PROCESS_GIT,
        implementer: &implementer,
        executor: &executor,
        workers: &WORKERS,
        health: &health,
        clock: &clock,
        sleeper: &sleeper,
        worker_recovery_max_wait_seconds: 40,
        worker_recovery_retry_delays_seconds: vec![7, 11, 13],
        worker_recovery_max_attempts: 4,
        state_root: fx.root.clone(),
        container_tracker: None,
    });
    runner.start(&campaign).unwrap();

    let completed = runner.run(&campaign.campaign_id).unwrap();

    assert_eq!(completed.state, CampaignState::Completed);
    assert_eq!(sleeper.sleeps(), vec![7, 11, 13]);
}

#[test]
fn load_state_migrates_historical_attempt_without_kind_from_verification_step() {
    let fx = fixture();
    let campaign = make_campaign(
        "compat",
        &fx.sha,
        vec![CampaignStep {
            id: "verify-alpha".to_string(),
            kind: CampaignStepKind::Verification,
            task: "Verify alpha.".to_string(),
            allowed_paths: vec!["src/".to_string()],
            required_changed_paths: vec![],
            acceptance: StepAcceptance {
                commands: vec![vec!["cargo".to_string(), "test".to_string()]],
                required_artifacts: vec![],
            },
            limits: StepLimits {
                timeout_seconds: 120,
                network: "disabled".to_string(),
            },
        }],
        default_policy(),
    );
    let campaign_dir = fx
        .root
        .join("state")
        .join("campaigns")
        .join(&campaign.campaign_id);
    fs::create_dir_all(&campaign_dir).unwrap();
    fs::write(
        campaign_dir.join("campaign.json"),
        serde_json::to_string_pretty(&campaign).unwrap() + "\n",
    )
    .unwrap();
    let state = serde_json::json!({
        "schema_version": "rack-ai/campaign/v1",
        "campaign_id": campaign.campaign_id,
        "campaign_digest": "digest",
        "repository_id": "fixture",
        "base_sha": fx.sha,
        "branch": "rack/campaign-compat",
        "worktree_path": fx.root.join("workspaces/compat/repo").display().to_string(),
        "current_head_sha": "head",
        "state": "blocked",
        "current_step_id": "verify-alpha",
        "current_attempt": 1,
        "pause_requested": false,
        "cancel_requested": false,
        "start_time": "1",
        "end_time": "2",
        "duration_seconds": 1,
        "remaining_seconds": 0,
        "last_heartbeat": "2",
        "steps": [{
            "step_id": "verify-alpha",
            "kind": "verification",
            "disposition": "accepted",
            "review_disposition": "accepted",
            "review_rationale": "ok",
            "attempts": [{
                "attempt": 1,
                "worker_id": "local-primary",
                "start_time": "1",
                "end_time": "2",
                "disposition": "accepted",
                "classification": null,
                "rationale": "verified",
                "commit_sha": null,
                "repair_instruction": null,
                "next_repair_instruction": null,
                "repair_of": null,
                "fallback_of": null
            }],
            "accepted_commit": null
        }],
        "revisions": [],
        "active_lease_id": null,
        "active_container_id": null,
        "error_message": null,
        "blocked_reason": "compat"
    });
    fs::write(
        campaign_dir.join("state.json"),
        serde_json::to_string_pretty(&state).unwrap() + "\n",
    )
    .unwrap();

    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, vec![]);
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 10);
    let loaded = runner.load_state(&campaign.campaign_id).unwrap().unwrap();

    assert_eq!(loaded.steps[0].attempts[0].kind, AttemptKind::Verification);
}

#[test]
fn supervisor_isolates_incompatible_campaign_state_and_continues() {
    let fx = fixture();
    let good_campaign = make_campaign(
        "good",
        &fx.sha,
        vec![sample_step("add-alpha", "src/alpha.rs", "Add alpha.")],
        default_policy(),
    );
    let good_dir = fx.root.join("state").join("campaigns").join("good");
    fs::create_dir_all(&good_dir).unwrap();
    fs::write(
        good_dir.join("campaign.json"),
        serde_json::to_string_pretty(&good_campaign).unwrap() + "\n",
    )
    .unwrap();
    fs::write(
        good_dir.join("state.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "rack-ai/campaign/v1",
            "campaign_id": "good",
            "campaign_digest": "digest",
            "repository_id": "fixture",
            "base_sha": fx.sha,
            "branch": "rack/campaign-good",
            "worktree_path": fx.root.join("workspaces/good/repo").display().to_string(),
            "current_head_sha": "head",
            "state": "completed",
            "current_step_id": null,
            "current_attempt": 1,
            "pause_requested": false,
            "cancel_requested": false,
            "start_time": "1",
            "end_time": "2",
            "duration_seconds": 1,
            "remaining_seconds": 0,
            "last_heartbeat": "2",
            "steps": [],
            "revisions": [],
            "active_lease_id": null,
            "active_container_id": null,
            "error_message": null,
            "blocked_reason": null
        }))
        .unwrap()
            + "\n",
    )
    .unwrap();

    let bad_dir = fx.root.join("state").join("campaigns").join("bad");
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(
        bad_dir.join("state.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "rack-ai/campaign/v1",
            "campaign_id": "bad",
            "campaign_digest": "digest",
            "repository_id": "fixture",
            "base_sha": fx.sha,
            "branch": "rack/campaign-bad",
            "worktree_path": fx.root.join("workspaces/bad/repo").display().to_string(),
            "current_head_sha": "head",
            "state": "running",
            "current_step_id": "mystery",
            "current_attempt": 1,
            "pause_requested": false,
            "cancel_requested": false,
            "start_time": "1",
            "end_time": null,
            "duration_seconds": 1,
            "remaining_seconds": 10,
            "last_heartbeat": "2",
            "steps": [{
                "step_id": "mystery",
                "disposition": "accepted",
                "attempts": [{
                    "attempt": 1,
                    "worker_id": "local-coder",
                    "start_time": "1",
                    "end_time": "2",
                    "disposition": "accepted",
                    "classification": null,
                    "rationale": "old",
                    "commit_sha": null
                }],
                "accepted_commit": null
            }],
            "revisions": [],
            "active_lease_id": null,
            "active_container_id": null,
            "error_message": null,
            "blocked_reason": null
        }))
        .unwrap()
            + "\n",
    )
    .unwrap();

    let executor = HostExecutor::new();
    let implementer = ScriptedChangeImplementer::new(&executor, vec![]);
    let runner = make_runner(
        &fx,
        &good_campaign,
        &implementer,
        &executor,
        &Healthy,
        10_000,
    );
    let supervisor = CampaignSupervisor::new(CampaignSupervisorDependencies {
        runner: &runner,
        clock: Box::leak(Box::new(TestClock {
            now: Cell::new(10_000),
        })),
        state_root: fx.root.clone(),
        workspace_root: fx.workspaces.clone(),
        operations: OperationsConfig {
            schema_version: "rack-ai/operations/v1".to_string(),
            supervisor: SupervisorConfig {
                scan_interval_seconds: 30,
                resume_running_campaigns: true,
                podman_command: "true".to_string(),
                worker_recovery_max_wait_seconds: 900,
                worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
                worker_recovery_max_attempts: 11,
            },
            retention: RetentionConfig {
                max_terminal_campaign_age_seconds: 3_600,
                retain_terminal_campaigns: 10,
                max_auxiliary_artifact_age_seconds: 3_600,
                retain_auxiliary_artifacts: 1,
            },
        },
    })
    .unwrap();

    let report = supervisor.run_once().unwrap();

    assert_eq!(report.scanned_campaigns, 2);
    assert!(
        report
            .actions
            .iter()
            .any(|item| item.campaign_id == "good" && item.action == "observe")
    );
    assert!(
        report
            .actions
            .iter()
            .any(|item| item.campaign_id == "bad" && item.action == "incompatible_state")
    );
    assert!(bad_dir.join("supervisor-load-error.json").exists());
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

#[test]
fn recovery_reasoner_repairs_same_strategy_after_local_defect() {
    let fx = cargo_fixture(compatibility_service_source());
    let executor = HostExecutor::with_process_commands();
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt(
                "src/service.rs",
                "pub struct AssessmentService;

impl AssessmentService {
    pub fn open_case(&self) -> u32 {
        let broken = ;
        broken
    }
}
",
            ),
            write_attempt(
                "src/service.rs",
                r#"pub struct AssessmentService;

impl AssessmentService {
    pub fn open_case(&self) -> u32 {
        2
    }
}
"#,
            ),
        ],
    );
    let campaign = make_campaign(
        "repair-same-strategy",
        &fx.sha,
        vec![compatibility_step(
            "Fix AssessmentService without changing src/main.rs.",
        )],
        default_policy(),
    );
    let reasoner = ScriptedRecoveryReasoner::new(vec![Ok(decision(
        RecoveryDecisionKind::Repair,
        RecoveryFailureKind::LocalImplementationDefect,
        RecoveryWorkerAction::SameWorker,
        Some("Repair the syntax error in src/service.rs without changing src/main.rs."),
    ))]);
    let reviewer = AcceptingReviewer;
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_recovery_reasoner(&reasoner)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Completed);
    assert_eq!(state.steps[0].attempts.len(), 2);
    assert_eq!(
        implementer.seen_workers(),
        vec!["local-coder", "local-coder"]
    );
    assert!(implementer.seen_tasks()[1].contains("syntax error"));
    assert_eq!(reasoner.calls(), 1);
}

#[test]
fn compatibility_failure_replans_within_scope_and_preserves_main() {
    let fx = cargo_fixture(compatibility_service_source());
    let executor = HostExecutor::with_process_commands();
    let broken = r#"pub struct AssessmentService;

impl AssessmentService {
    pub fn open_case(&self, resident: &str) -> u32 {
        resident.len() as u32
    }
}
"#;
    let repaired = r#"pub struct AssessmentService;

impl AssessmentService {
    pub fn open_case(&self) -> u32 {
        self.open_case_for("default")
    }

    pub fn open_case_for(&self, resident: &str) -> u32 {
        resident.len() as u32
    }
}
"#;
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt("src/service.rs", broken),
            write_attempt("src/service.rs", repaired),
        ],
    );
    let task = "Extend AssessmentService, but preserve the existing CLI caller in src/main.rs and do not modify src/main.rs.";
    let campaign = make_campaign(
        "compatibility-replan",
        &fx.sha,
        vec![compatibility_step(task)],
        default_policy(),
    );
    let reasoner = ScriptedRecoveryReasoner::new(vec![Ok(decision(
        RecoveryDecisionKind::Replan,
        RecoveryFailureKind::CompatibilityConstraint,
        RecoveryWorkerAction::SameWorker,
        Some(
            "Preserve the existing src/main.rs caller contract. Revise only src/service.rs inside allowed_paths.",
        ),
    ))]);
    let reviewer = AcceptingReviewer;
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_recovery_reasoner(&reasoner)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Completed);
    assert_eq!(state.steps[0].attempts.len(), 2);
    assert_eq!(
        fs::read_to_string(Path::new(&state.worktree_path).join("src/main.rs")).unwrap(),
        r#"mod service;

use service::AssessmentService;

fn main() {
    let service = AssessmentService;
    println!("{}", service.open_case());
}
"#
    );
    let recovery = fs::read_to_string(
        runner
            .attempt_dir(&campaign.campaign_id, "service", 1)
            .join("recovery-decision.json"),
    )
    .unwrap();
    assert!(recovery.contains(r#""kind": "replan""#));
    assert!(implementer.seen_tasks()[1].contains("src/main.rs"));
    let request = &reasoner.requests()[0];
    assert!(request.prompt().contains("compatibility"));
}

#[test]
fn insufficient_authority_decision_blocks_safely() {
    let fx = cargo_fixture(compatibility_service_source());
    let executor = HostExecutor::with_process_commands();
    let broken = r#"pub struct AssessmentService;

impl AssessmentService {
    pub fn open_case(&self, resident: &str) -> u32 {
        resident.len() as u32
    }
}
"#;
    let implementer =
        ScriptedChangeImplementer::new(&executor, vec![write_attempt("src/service.rs", broken)]);
    let campaign = make_campaign(
        "insufficient-authority",
        &fx.sha,
        vec![compatibility_step("Change AssessmentService if possible.")],
        default_policy(),
    );
    let reasoner = ScriptedRecoveryReasoner::new(vec![Ok(decision(
        RecoveryDecisionKind::BlockInsufficientAuthority,
        RecoveryFailureKind::InsufficientAuthority,
        RecoveryWorkerAction::SameWorker,
        None,
    ))]);
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_recovery_reasoner(&reasoner);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Blocked);
    assert_eq!(state.steps[0].attempts.len(), 1);
    assert_eq!(
        state.steps[0].attempts[0].classification,
        Some(FailureClassification::InsufficientAuthority)
    );
}

#[test]
fn repeated_equivalent_failure_forces_fallback_replan() {
    let fx = cargo_fixture(compatibility_service_source());
    let executor = HostExecutor::with_process_commands();
    let broken = r#"pub struct AssessmentService;

impl AssessmentService {
    pub fn open_case(&self, resident: &str) -> u32 {
        resident.len() as u32
    }
}
"#;
    let repaired = r#"pub struct AssessmentService;

impl AssessmentService {
    pub fn open_case(&self) -> u32 {
        self.open_case_for("default")
    }

    pub fn open_case_for(&self, resident: &str) -> u32 {
        resident.len() as u32
    }
}
"#;
    let implementer = ScriptedChangeImplementer::new(
        &executor,
        vec![
            write_attempt("src/service.rs", broken),
            write_attempt("src/service.rs", broken),
            write_attempt("src/service.rs", repaired),
        ],
    );
    let campaign = make_campaign(
        "stagnation-fallback",
        &fx.sha,
        vec![compatibility_step(
            "Extend AssessmentService without touching src/main.rs.",
        )],
        default_policy(),
    );
    let reasoner = ScriptedRecoveryReasoner::new(vec![
        Ok(decision(
            RecoveryDecisionKind::Repair,
            RecoveryFailureKind::StrategyFailure,
            RecoveryWorkerAction::SameWorker,
            Some("Try again in src/service.rs."),
        )),
        Ok(decision(
            RecoveryDecisionKind::Repair,
            RecoveryFailureKind::StrategyFailure,
            RecoveryWorkerAction::SameWorker,
            Some("Try again in src/service.rs."),
        )),
    ]);
    let reviewer = AcceptingReviewer;
    let runner = make_runner(&fx, &campaign, &implementer, &executor, &Healthy, 1_000)
        .with_recovery_reasoner(&reasoner)
        .with_reviewer(&reviewer);
    runner.start(&campaign).unwrap();
    let state = runner.run(&campaign.campaign_id).unwrap();
    assert_eq!(state.state, CampaignState::Completed);
    assert_eq!(
        implementer.seen_workers(),
        vec!["local-coder", "local-coder", "local-primary"]
    );
    assert!(implementer.seen_tasks()[2].contains("Replan the implementation"));
    let recovery = fs::read_to_string(
        runner
            .attempt_dir(&campaign.campaign_id, "service", 2)
            .join("recovery-decision.json"),
    )
    .unwrap();
    assert!(recovery.contains(r#""repeated_failure_count": 1"#));
    assert!(recovery.contains(r#""kind": "replan""#));
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
        sleeper: &NOOP_SLEEPER,
        worker_recovery_max_wait_seconds: 900,
        worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
        worker_recovery_max_attempts: 11,
        state_root: fx.root.clone(),
        container_tracker: None,
    })
}

fn make_runner_with_support<'a>(
    fx: &'a Fixture,
    _campaign: &Campaign,
    implementer: &'a ScriptedChangeImplementer<'a>,
    executor: &'a HostExecutor,
    health: &'a dyn CampaignHealth,
    clock: &'a dyn UnixClock,
    sleeper: &'a dyn RecoverySleeper,
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
        clock,
        sleeper,
        worker_recovery_max_wait_seconds: 900,
        worker_recovery_retry_delays_seconds: vec![5, 10, 15, 20, 30, 45, 60, 90, 120, 120],
        worker_recovery_max_attempts: 11,
        state_root: fx.root.clone(),
        container_tracker: None,
    })
}
