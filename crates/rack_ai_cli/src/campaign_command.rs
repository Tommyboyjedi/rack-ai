use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use rack_ai_application::Campaign;
use rack_ai_application::CampaignEvent;
use rack_ai_application::CampaignRunner;
use rack_ai_application::CampaignState::Completed;
use rack_ai_application::CampaignState::Failed;
use rack_ai_application::CampaignState::Running;
use rack_ai_application::CampaignStep;
use rack_ai_application::CampaignStepKind;
use rack_ai_application::ChangeImplementer;
use rack_ai_application::ChangeLayout;
use rack_ai_application::CommandEvidence;
use rack_ai_application::GitWorktree;
use rack_ai_application::ImplementChangeRequest;
use rack_ai_application::ReadFileRequest;
use rack_ai_application::RepositoryRegistry;
use rack_ai_application::RunCommandRequest;
use rack_ai_application::StepAttemptRecord;
use rack_ai_application::StepStatusRecord;
use rack_ai_application::WorkspaceExecutor;
use rack_ai_application::WorkspacePath;
use rack_ai_domain::AcceptanceCommand;
use rack_ai_domain::AllowedPath;
use rack_ai_domain::AllowedPaths;
use rack_ai_domain::ChangeStatus;
use rack_ai_domain::GitSha;
use rack_ai_infrastructure::EndpointProbe;
use rack_ai_infrastructure::FileSystemRepositoryRegistry;
use rack_ai_infrastructure::FileSystemRegistryRepository;
use rack_ai_infrastructure::GitCommand;
use rack_ai_infrastructure::GitCommandWorktree;
use rack_ai_infrastructure::PodmanAvailability;
use rack_ai_infrastructure::PodmanChangeImplementer;
use rack_ai_infrastructure::PodmanWorkspaceExecutor;
use rack_ai_infrastructure::RegistryPaths;

#[derive(serde::Serialize)]
struct AttemptReview {
    step_id: String,
    attempt: usize,
    worker_id: String,
    status: String,
    rationale: String,
    changed_paths: Vec<String>,
    commands: Vec<CommandEvidence>,
    implementer_output: Option<String>,
    commit_sha: Option<String>,
}

struct WorkerRuntime {
    worker_id: String,
    endpoint: String,
    api_model_id: String,
}

struct AttemptOutcome {
    record: StepAttemptRecord,
    review: AttemptReview,
    accepted: bool,
}

pub fn run(repo_root: PathBuf, state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let command = arguments.first().ok_or("expected campaign subcommand")?;
    match command.as_str() {
        "validate" => validate_command(repo_root, state_root, &arguments[1..]),
        "start" => start_command(repo_root, state_root, &arguments[1..]),
        "runner" => runner_command(repo_root, state_root, &arguments[1..]),
        "status" => status_command(state_root, &arguments[1..]),
        "events" => events_command(state_root, &arguments[1..]),
        "inspect" => inspect_command(state_root, &arguments[1..]),
        value => Err(format!("unsupported campaign subcommand: {value}")),
    }
}

fn validate_command(repo_root: PathBuf, state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign = load_campaign_file(arguments.first().ok_or("expected campaign path")?)?;
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root.clone()));
    let policy = registry.command_policy()?;
    let git = GitCommandWorktree;
    let runner = CampaignRunner::new(&registry, &policy, &git, state_root);
    runner.validate(&campaign)?;
    validate_worker_runtime(&repo_root, &campaign)?;
    println!("campaign_id: {}", campaign.campaign_id);
    println!("status: valid");
    Ok(0)
}

fn start_command(repo_root: PathBuf, state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign = load_campaign_file(arguments.first().ok_or("expected campaign path")?)?;
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root.clone()));
    let policy = registry.command_policy()?;
    let git = GitCommandWorktree;
    let runner = CampaignRunner::new(&registry, &policy, &git, state_root.clone());
    runner.start(&campaign)?;
    execute_campaign(repo_root, state_root, campaign.campaign_id.as_str())
}

fn runner_command(repo_root: PathBuf, state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    execute_campaign(repo_root, state_root, campaign_id)
}

fn status_command(state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let emit_json = arguments.iter().any(|value| value == "--emit-json");
    let path = state_root
        .join("state")
        .join("campaigns")
        .join(campaign_id)
        .join("state.json");
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    if emit_json {
        println!("{content}");
    } else {
        let state: rack_ai_application::CampaignStatus =
            serde_json::from_str(&content).map_err(|error| error.to_string())?;
        println!("campaign_id: {}", state.campaign_id);
        println!("state: {:?}", state.state);
        println!("branch: {}", state.branch);
        println!("worktree: {}", state.worktree_path);
        println!("head_sha: {}", state.current_head_sha);
        if let Some(step) = state.current_step_id {
            println!("current_step: {step}");
        }
        if let Some(error) = state.error_message {
            println!("last_error: {error}");
        }
    }
    Ok(0)
}

fn events_command(state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let follow = arguments.iter().any(|value| value == "--follow");
    let path = state_root
        .join("state")
        .join("campaigns")
        .join(campaign_id)
        .join("events.jsonl");
    let mut last_len = 0usize;
    loop {
        let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        print!("{}", &content[last_len..]);
        last_len = content.len();
        if !follow {
            break;
        }
        thread::sleep(Duration::from_secs(1));
    }
    Ok(0)
}

fn inspect_command(state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let root = state_root.join("state").join("campaigns").join(campaign_id);
    println!("campaign_dir: {}", root.display());
    for path in walk_files(root.join("steps"))? {
        println!("{}", path.display());
    }
    Ok(0)
}

fn execute_campaign(repo_root: PathBuf, state_root: PathBuf, campaign_id: &str) -> Result<i32, String> {
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root.clone()));
    let policy = registry.command_policy()?;
    let git = GitCommandWorktree;
    let runner = CampaignRunner::new(&registry, &policy, &git, state_root.clone());
    let mut state = runner
        .load_state(campaign_id)?
        .ok_or(format!("campaign state not found: {campaign_id}"))?;
    let campaign = runner.load_campaign(campaign_id)?;
    validate_worker_runtime(&repo_root, &campaign)?;
    let executor = PodmanWorkspaceExecutor::new(registry.executor_config()?);
    let implementer = PodmanChangeImplementer::new(PodmanWorkspaceExecutor::new(
        registry.executor_config()?,
    ));
    let mut total_attempts = state
        .steps
        .iter()
        .map(|step| step.attempts.len())
        .sum::<usize>();
    state.state = Running;
    state.error_message = None;
    state.blocked_reason = None;
    state.last_heartbeat = now_text();
    runner.save_state(&state)?;
    for step in &campaign.steps {
        let step_index = find_step_status_index(&state.steps, &step.id)?;
        if state.steps[step_index].disposition == "accepted" {
            continue;
        }
        state.current_step_id = Some(step.id.clone());
        runner.save_state(&state)?;
        runner.log_event(new_event(
            campaign_id,
            Some(step.id.as_str()),
            None,
            "step_started",
            format!("starting step {}", step.id),
        ))?;
        let outcome = match step.kind {
            CampaignStepKind::Implementation => execute_implementation_step(
                &repo_root,
                &campaign,
                &state,
                step,
                &executor,
                &implementer,
                &git,
                &runner,
                &mut total_attempts,
            )?,
            CampaignStepKind::Verification => execute_verification_step(
                &campaign,
                &state,
                step,
                &executor,
                &git,
                &runner,
                &mut total_attempts,
            )?,
        };
        let attempt_number = state.steps[step_index].attempts.len() + 1;
        let mut record = outcome.record;
        record.attempt = attempt_number;
        let mut review = outcome.review;
        review.step_id = step.id.clone();
        review.attempt = attempt_number;
        state.steps[step_index].attempts.push(record);
        persist_review(&runner, campaign_id, step.id.as_str(), attempt_number, &review)?;
        if outcome.accepted {
            state.steps[step_index].disposition = "accepted".to_string();
            state.steps[step_index].accepted_commit = review.commit_sha.clone();
            if let Some(commit_sha) = &review.commit_sha {
                state.current_head_sha = commit_sha.clone();
            }
            state.last_heartbeat = now_text();
            runner.save_state(&state)?;
            runner.log_event(new_event(
                campaign_id,
                Some(step.id.as_str()),
                Some(attempt_number),
                "step_accepted",
                format!("accepted step {}", step.id),
            ))?;
            continue;
        }
        state.steps[step_index].disposition = "failed".to_string();
        state.state = Failed;
        state.error_message = Some(review.rationale.clone());
        state.last_heartbeat = now_text();
        state.end_time = Some(now_text());
        runner.save_state(&state)?;
        runner.log_event(new_event(
            campaign_id,
            Some(step.id.as_str()),
            Some(attempt_number),
            "step_failed",
            review.rationale,
        ))?;
        return Ok(1);
    }
    state.state = Completed;
    state.current_step_id = None;
    state.end_time = Some(now_text());
    state.last_heartbeat = now_text();
    runner.save_state(&state)?;
    runner.log_event(new_event(
        campaign_id,
        None,
        None,
        "campaign_completed",
        "campaign completed".to_string(),
    ))?;
    println!("campaign_id: {campaign_id}");
    println!("state: completed");
    Ok(0)
}

fn execute_implementation_step(
    repo_root: &Path,
    campaign: &Campaign,
    state: &rack_ai_application::CampaignStatus,
    step: &CampaignStep,
    executor: &PodmanWorkspaceExecutor,
    implementer: &PodmanChangeImplementer,
    git: &GitCommandWorktree,
    runner: &CampaignRunner<'_>,
    total_attempts: &mut usize,
) -> Result<AttemptOutcome, String> {
    let attempts = build_worker_attempts(campaign);
    let planned_attempts = attempts.len();
    let worktree_path = PathBuf::from(state.worktree_path.clone());
    let mut previous_error = String::new();
    for (attempt_index, worker_id) in attempts.into_iter().enumerate() {
        if *total_attempts >= campaign.limits.max_total_attempts {
            break;
        }
        *total_attempts += 1;
        let runtime = resolve_worker_runtime(repo_root, worker_id.as_str())?;
        let start = now_text();
        let task = task_for_attempt(step, previous_error.as_str(), attempt_index);
        let request = ImplementChangeRequest::new(worktree_path.clone(), task)
            .with_policy(build_allowed_paths(&step.allowed_paths)?, step.limits.timeout_seconds as u32)
            .with_max_turns(ChangeLayout::coder_max_turns())
            .with_worker(
                runtime.worker_id.clone(),
                runtime.endpoint.clone(),
                runtime.api_model_id.clone(),
            );
        let implementer_output = match implementer.implement(&request) {
            Ok(result) => result.output().to_string(),
            Err(error) => {
                previous_error = error;
                clean_worktree(&worktree_path)?;
                if attempt_index + 1 < planned_attempts {
                    continue;
                }
                return Ok(rejected_attempt(
                    runtime.worker_id,
                    start,
                    "failed".to_string(),
                    previous_error,
                    Vec::new(),
                    Vec::new(),
                    None,
                    None,
                ));
            }
        };
        let evidence = git.inspect(&rack_ai_application::InspectChangeWorktreeRequest::new(
            worktree_path.clone(),
            GitSha::new(state.current_head_sha.clone())?,
        ))?;
        let changed_paths = evidence.changed_paths().to_vec();
        if changed_paths.is_empty() {
            previous_error = "worker reported completion without changing files".to_string();
            clean_worktree(&worktree_path)?;
            if attempt_index + 1 < planned_attempts {
                continue;
            }
            return Ok(rejected_attempt(
                runtime.worker_id,
                start,
                "no_change".to_string(),
                previous_error,
                changed_paths,
                Vec::new(),
                Some(implementer_output),
                None,
            ));
        }
        if let Err(error) = assert_step_paths(step, &changed_paths) {
            previous_error = error;
            clean_worktree(&worktree_path)?;
            if attempt_index + 1 < planned_attempts {
                continue;
            }
            return Ok(rejected_attempt(
                runtime.worker_id,
                start,
                "path_policy_failed".to_string(),
                previous_error,
                changed_paths,
                Vec::new(),
                Some(implementer_output),
                None,
            ));
        }
        let commands = match run_acceptance_commands(executor, &worktree_path, step) {
            Ok(value) => value,
            Err(error) => {
                previous_error = error;
                clean_worktree(&worktree_path)?;
                if attempt_index + 1 < planned_attempts {
                    continue;
                }
                return Ok(rejected_attempt(
                    runtime.worker_id,
                    start,
                    "checks_failed".to_string(),
                    previous_error,
                    changed_paths,
                    Vec::new(),
                    Some(implementer_output),
                    None,
                ));
            }
        };
        let commit_sha = commit_step(&worktree_path, campaign, step, &changed_paths)?;
        return Ok(accepted_attempt(
            runtime.worker_id,
            start,
            changed_paths,
            commands,
            Some(implementer_output),
            Some(commit_sha),
        ));
    }
    runner.log_event(new_event(
        campaign.campaign_id.as_str(),
        Some(step.id.as_str()),
        None,
        "attempts_exhausted",
        format!("exhausted worker attempts for step {}", step.id),
    ))?;
    Ok(rejected_attempt(
        campaign.worker_policy.fallback.clone(),
        now_text(),
        "failed".to_string(),
        format!("no worker attempt succeeded for step {}", step.id),
        Vec::new(),
        Vec::new(),
        None,
        None,
    ))
}

fn execute_verification_step(
    campaign: &Campaign,
    state: &rack_ai_application::CampaignStatus,
    step: &CampaignStep,
    executor: &PodmanWorkspaceExecutor,
    _git: &GitCommandWorktree,
    _runner: &CampaignRunner<'_>,
    total_attempts: &mut usize,
) -> Result<AttemptOutcome, String> {
    if *total_attempts >= campaign.limits.max_total_attempts {
        return Ok(rejected_attempt(
            campaign.worker_policy.primary.clone(),
            now_text(),
            "failed".to_string(),
            "campaign attempt budget exhausted".to_string(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        ));
    }
    *total_attempts += 1;
    let worktree_path = PathBuf::from(state.worktree_path.clone());
    let start = now_text();
    let commands = match run_acceptance_commands(executor, &worktree_path, step) {
        Ok(value) => value,
        Err(error) => {
            return Ok(rejected_attempt(
                campaign.worker_policy.primary.clone(),
                start,
                "checks_failed".to_string(),
                error,
                Vec::new(),
                Vec::new(),
                None,
                None,
            ));
        }
    };
    Ok(accepted_attempt(
        campaign.worker_policy.primary.clone(),
        start,
        Vec::new(),
        commands,
        None,
        None,
    ))
}

fn run_acceptance_commands(
    executor: &PodmanWorkspaceExecutor,
    worktree_path: &Path,
    step: &CampaignStep,
) -> Result<Vec<CommandEvidence>, String> {
    let timeout = step.limits.timeout_seconds as u32;
    let mut commands = Vec::new();
    for argv in &step.acceptance.commands {
        let command = AcceptanceCommand::new(argv.clone())?;
        let result = executor.run_command(
            &RunCommandRequest::new(worktree_path.to_path_buf(), command.argv().to_vec())?
                .with_timeout_seconds(timeout),
        )?;
        let evidence = result.evidence().clone();
        if !evidence.succeeded() {
            return Err(format!(
                "acceptance command failed: {}",
                evidence.argv().join(" ")
            ));
        }
        commands.push(evidence);
    }
    for artifact in &step.acceptance.required_artifacts {
        executor.read_file(&ReadFileRequest::new(
            worktree_path.to_path_buf(),
            WorkspacePath::parse(artifact.as_str())?,
        ))?;
    }
    Ok(commands)
}

fn build_worker_attempts(campaign: &Campaign) -> Vec<String> {
    let mut attempts = Vec::new();
    for _ in 0..campaign.worker_policy.primary_attempts {
        attempts.push(campaign.worker_policy.primary.clone());
    }
    for _ in 0..campaign.worker_policy.repair_attempts {
        attempts.push(campaign.worker_policy.primary.clone());
    }
    for _ in 0..campaign.worker_policy.fallback_attempts {
        attempts.push(campaign.worker_policy.fallback.clone());
    }
    attempts
}

fn task_for_attempt(step: &CampaignStep, previous_error: &str, attempt_index: usize) -> String {
    if attempt_index == 0 || previous_error.is_empty() {
        return step.task.clone();
    }
    format!(
        "{}\n\nRepair the previous failed attempt. Stay within the same allowed paths.\nFailure context: {}",
        step.task, previous_error
    )
}

fn resolve_worker_runtime(repo_root: &Path, worker_id: &str) -> Result<WorkerRuntime, String> {
    let repository = FileSystemRegistryRepository::new(RegistryPaths::new(repo_root.to_path_buf()));
    let worker = repository
        .load_workers()?
        .into_iter()
        .find(|item| item.id == worker_id)
        .ok_or(format!("unknown worker: {worker_id}"))?;
    if !worker.enabled {
        return Err(format!("worker disabled: {worker_id}"));
    }
    let models = repository.load_models()?;
    let model_by_worker = models
        .into_iter()
        .find(|item| item.worker_id == worker_id && item.status == "active")
        .ok_or(format!("no active model bound to worker: {worker_id}"))?;
    Ok(WorkerRuntime {
        worker_id: worker_id.to_string(),
        endpoint: model_by_worker.endpoint,
        api_model_id: worker_id.to_string(),
    })
}

fn validate_worker_runtime(repo_root: &Path, campaign: &Campaign) -> Result<(), String> {
    let probe = EndpointProbe;
    for worker_id in [
        campaign.worker_policy.primary.as_str(),
        campaign.worker_policy.fallback.as_str(),
    ] {
        let runtime = resolve_worker_runtime(repo_root, worker_id)?;
        if !probe.check_models(runtime.endpoint.as_str())? {
            return Err(format!("worker endpoint is unhealthy: {worker_id}"));
        }
    }
    PodmanAvailability::ensure()?;
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root.to_path_buf()));
    let config = registry.executor_config()?;
    PodmanAvailability::ensure_image("podman", config.image())?;
    Ok(())
}

fn build_allowed_paths(values: &[String]) -> Result<AllowedPaths, String> {
    AllowedPaths::new(
        values
            .iter()
            .cloned()
            .map(AllowedPath::new)
            .collect::<Result<Vec<_>, _>>()?,
    )
}

fn assert_step_paths(step: &CampaignStep, changed_paths: &[String]) -> Result<(), String> {
    let allowed = build_allowed_paths(&step.allowed_paths)?;
    let disallowed = allowed.reject_disallowed(changed_paths);
    if !disallowed.is_empty() {
        return Err(format!(
            "changed paths outside allowed_paths: {}",
            disallowed
                .iter()
                .map(|path| path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for required in &step.required_changed_paths {
        if !changed_paths.iter().any(|path| path.starts_with(required)) {
            return Err(format!("required changed path not satisfied: {required}"));
        }
    }
    Ok(())
}

fn commit_step(
    worktree_path: &Path,
    campaign: &Campaign,
    step: &CampaignStep,
    changed_paths: &[String],
) -> Result<String, String> {
    if !campaign.allow_local_commits {
        return Err("campaign does not allow local commits".to_string());
    }
    let git_status = GitCommand::run(worktree_path, &["status", "--porcelain"])?;
    if git_status.trim().is_empty() {
        return Err("cannot commit an empty diff".to_string());
    }
    let mut add_args = vec!["add".to_string(), "-A".to_string(), "--".to_string()];
    add_args.extend(changed_paths.iter().cloned());
    run_git_owned(worktree_path, add_args)?;
    run_git_owned(
        worktree_path,
        vec![
            "-c".to_string(),
            "user.name=Rack AI".to_string(),
            "-c".to_string(),
            "user.email=rack-ai@local".to_string(),
            "commit".to_string(),
            "-m".to_string(),
            format!("rack({}): {}", campaign.campaign_id, step.id),
        ],
    )?;
    GitCommand::run(worktree_path, &["rev-parse", "HEAD"])
}

fn clean_worktree(worktree_path: &Path) -> Result<(), String> {
    let _ = GitCommand::run(worktree_path, &["reset", "--hard", "HEAD"])?;
    let _ = GitCommand::run(worktree_path, &["clean", "-fd"])?;
    Ok(())
}

fn run_git_owned(worktree_path: &Path, args: Vec<String>) -> Result<String, String> {
    let output = Command::new("git")
        .args(args.iter().map(|item| item.as_str()))
        .current_dir(worktree_path)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn find_step_status_index(steps: &[StepStatusRecord], step_id: &str) -> Result<usize, String> {
    steps
        .iter()
        .position(|step| step.step_id == step_id)
        .ok_or(format!("missing step status: {step_id}"))
}

fn persist_review(
    runner: &CampaignRunner<'_>,
    campaign_id: &str,
    step_id: &str,
    attempt: usize,
    review: &AttemptReview,
) -> Result<(), String> {
    let dir = runner.attempt_dir(campaign_id, step_id, attempt);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(review).map_err(|error| error.to_string())?;
    fs::write(dir.join("review-packet.json"), format!("{json}\n"))
        .map_err(|error| error.to_string())?;
    let transcript = serde_json::json!({
        "worker_id": review.worker_id,
        "output": review.implementer_output,
    });
    fs::write(
        dir.join("worker-transcript.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&transcript).map_err(|error| error.to_string())?
        ),
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        dir.join("command-evidence.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&review.commands).map_err(|error| error.to_string())?
        ),
    )
    .map_err(|error| error.to_string())
}

fn accepted_attempt(
    worker_id: String,
    start_time: String,
    changed_paths: Vec<String>,
    commands: Vec<CommandEvidence>,
    implementer_output: Option<String>,
    commit_sha: Option<String>,
) -> AttemptOutcome {
    let end_time = now_text();
    AttemptOutcome {
        record: StepAttemptRecord {
            attempt: 0,
            worker_id: worker_id.clone(),
            start_time: start_time.clone(),
            end_time: end_time.clone(),
            disposition: "accepted".to_string(),
            rationale: "acceptance checks passed".to_string(),
            commit_sha: commit_sha.clone(),
            repair_instruction: None,
        },
        review: AttemptReview {
            step_id: String::new(),
            attempt: 0,
            worker_id,
            status: serde_json::to_value(ChangeStatus::ChecksPassed)
                .ok()
                .and_then(|value| value.as_str().map(|text| text.to_string()))
                .unwrap_or_else(|| "checks_passed".to_string()),
            rationale: "acceptance checks passed".to_string(),
            changed_paths,
            commands,
            implementer_output,
            commit_sha,
        },
        accepted: true,
    }
}

fn rejected_attempt(
    worker_id: String,
    start_time: String,
    disposition: String,
    rationale: String,
    changed_paths: Vec<String>,
    commands: Vec<CommandEvidence>,
    implementer_output: Option<String>,
    commit_sha: Option<String>,
) -> AttemptOutcome {
    let end_time = now_text();
    AttemptOutcome {
        record: StepAttemptRecord {
            attempt: 0,
            worker_id: worker_id.clone(),
            start_time: start_time.clone(),
            end_time: end_time.clone(),
            disposition: disposition.clone(),
            rationale: rationale.clone(),
            commit_sha: commit_sha.clone(),
            repair_instruction: None,
        },
        review: AttemptReview {
            step_id: String::new(),
            attempt: 0,
            worker_id,
            status: disposition,
            rationale,
            changed_paths,
            commands,
            implementer_output,
            commit_sha,
        },
        accepted: false,
    }
}

fn new_event(
    campaign_id: &str,
    step_id: Option<&str>,
    attempt: Option<usize>,
    event_type: &str,
    message: String,
) -> CampaignEvent {
    CampaignEvent {
        timestamp: now_text(),
        campaign_id: campaign_id.to_string(),
        step_id: step_id.map(|value| value.to_string()),
        attempt,
        event_type: event_type.to_string(),
        message,
        details: serde_json::Map::new(),
    }
}

fn load_campaign_file(path: &str) -> Result<Campaign, String> {
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

fn now_text() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn walk_files(root: PathBuf) -> Result<Vec<PathBuf>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let mut stack = vec![root];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}
