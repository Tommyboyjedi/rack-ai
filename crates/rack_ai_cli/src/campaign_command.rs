use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rack_ai_application::Campaign;
use rack_ai_application::CampaignContainerTracker;
use rack_ai_application::CampaignEvent;
use rack_ai_application::CampaignHealth;
use rack_ai_application::CampaignRevisionDocument;
use rack_ai_application::CampaignRunner;
use rack_ai_application::CampaignRunnerDependencies;
use rack_ai_application::CampaignStatus;
use rack_ai_application::CampaignSupervisionReport;
use rack_ai_application::CampaignSupervisor;
use rack_ai_application::CampaignSupervisorDependencies;
use rack_ai_application::CampaignWorkerCatalog;
use rack_ai_application::CampaignWorkerRuntime;
use rack_ai_application::ChangeImplementer;
use rack_ai_application::OperationsConfig;
use rack_ai_application::RepositoryRegistry;
use rack_ai_application::ScriptedChangeImplementer;
use rack_ai_application::ScriptedImplementerDocument;
use rack_ai_application::SystemRecoverySleeper;
use rack_ai_application::SystemUnixClock;
use rack_ai_infrastructure::ConfiguredWorkspaceExecutor;
use rack_ai_infrastructure::EndpointProbe;
use rack_ai_infrastructure::FileSystemRegistryRepository;
use rack_ai_infrastructure::FileSystemRepositoryRegistry;
use rack_ai_infrastructure::GitCommandWorktree;
use rack_ai_infrastructure::JCodeChangeImplementer;
use rack_ai_infrastructure::LocalPrimaryRecoveryReasoner;
use rack_ai_infrastructure::LocalPrimaryReviewer;
use rack_ai_infrastructure::PodmanAvailability;
use rack_ai_infrastructure::RegistryPaths;

struct RegistryWorkers {
    repo_root: PathBuf,
}

impl CampaignWorkerCatalog for RegistryWorkers {
    fn runtime(&self, worker_id: &str) -> Result<CampaignWorkerRuntime, String> {
        let repository =
            FileSystemRegistryRepository::new(RegistryPaths::new(self.repo_root.clone()));
        let worker = repository
            .load_workers()?
            .into_iter()
            .find(|item| item.id == worker_id)
            .ok_or_else(|| format!("unknown worker: {worker_id}"))?;
        if !worker.enabled {
            return Err(format!("worker disabled: {worker_id}"));
        }
        let models = repository.load_models()?;
        let model = models
            .into_iter()
            .find(|item| item.worker_id == worker_id && item.status == "active")
            .ok_or_else(|| format!("no active model bound to worker: {worker_id}"))?;
        Ok(CampaignWorkerRuntime {
            worker_id: worker_id.to_string(),
            endpoint: model.endpoint,
            api_model_id: model
                .api_model_id
                .ok_or_else(|| format!("worker missing api_model_id binding: {worker_id}"))?,
            entrypoint: worker.entrypoint,
            provider_profile: worker
                .provider_profile
                .ok_or_else(|| format!("worker missing provider_profile: {worker_id}"))?,
            tool_profile: worker.tool_profile,
            context_window: model.context_window,
        })
    }
}

struct LiveHealth {
    repo_root: PathBuf,
}

impl CampaignHealth for LiveHealth {
    fn assert_workers(&self, primary: &str, fallback: &str) -> Result<(), String> {
        self.assert_worker(primary)?;
        self.assert_worker(fallback)
    }

    fn assert_worker(&self, worker_id: &str) -> Result<(), String> {
        let probe = EndpointProbe;
        let workers = RegistryWorkers {
            repo_root: self.repo_root.clone(),
        };
        let runtime = workers.runtime(worker_id)?;
        if !probe.check_models(runtime.endpoint.as_str())? {
            return Err(format!("worker endpoint is unhealthy: {worker_id}"));
        }
        Ok(())
    }

    fn assert_executor(&self) -> Result<(), String> {
        let registry =
            FileSystemRepositoryRegistry::new(RegistryPaths::new(self.repo_root.clone()));
        let config = registry.executor_config()?;
        match config.backend() {
            "host" => Ok(()),
            "podman" => {
                PodmanAvailability::ensure()?;
                let image = config
                    .image()
                    .ok_or("podman executor image is not configured".to_string())?;
                PodmanAvailability::ensure_image("podman", image)
            }
            value => Err(format!("unsupported executor backend: {value}")),
        }
    }
}

struct PermissiveHealth;

impl CampaignHealth for PermissiveHealth {
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

pub fn run(repo_root: PathBuf, state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let command = arguments.first().ok_or("expected campaign subcommand")?;
    match command.as_str() {
        "validate" => validate_command(repo_root, state_root, &arguments[1..]),
        "start" => start_command(repo_root, state_root, &arguments[1..]),
        "runner" => runner_command(repo_root, state_root, &arguments[1..]),
        "status" => status_command(state_root, &arguments[1..]),
        "events" => events_command(state_root, &arguments[1..]),
        "pause" => control_command(repo_root, state_root, &arguments[1..], Control::Pause),
        "resume" => resume_command(repo_root, state_root, &arguments[1..]),
        "cancel" => cancel_command(repo_root, state_root, &arguments[1..]),
        "revise" => revise_command(repo_root, state_root, &arguments[1..]),
        "inspect" => inspect_command(state_root, &arguments[1..]),
        "supervise" => supervise_command(repo_root, state_root, &arguments[1..]),
        value => Err(format!("unsupported campaign subcommand: {value}")),
    }
}

enum Control {
    Pause,
}

fn validate_command(
    repo_root: PathBuf,
    state_root: PathBuf,
    arguments: &[String],
) -> Result<i32, String> {
    let campaign = load_campaign_file(arguments.first().ok_or("expected campaign path")?)?;
    let seams = test_seams_from_arguments(arguments)?;
    with_runner(
        repo_root,
        state_root,
        seams.fixture.as_deref(),
        seams.skip_live,
        |runner| {
            runner.validate(&campaign)?;
            println!("campaign_id: {}", campaign.campaign_id);
            println!("status: valid");
            Ok(0)
        },
    )
}

fn start_command(
    repo_root: PathBuf,
    state_root: PathBuf,
    arguments: &[String],
) -> Result<i32, String> {
    let campaign_path = arguments.first().ok_or("expected campaign path")?;
    let campaign = load_campaign_file(campaign_path)?;
    let detach = arguments.iter().any(|value| value == "--detach");
    let seams = test_seams_from_arguments(arguments)?;
    if detach {
        ensure_detach_preflight()?;
    }
    with_runner(
        repo_root.clone(),
        state_root.clone(),
        seams.fixture.as_deref(),
        seams.skip_live,
        |runner| {
            runner.start(&campaign)?;
            Ok(0)
        },
    )?;
    if detach {
        return match spawn_detached_runner(&repo_root, &state_root, &campaign.campaign_id) {
            Ok(()) => {
                println!("campaign_id: {}", campaign.campaign_id);
                println!("status: detached");
                println!("unit: rack-ai-campaign-{}", campaign.campaign_id);
                Ok(0)
            }
            Err(error) => {
                let _ = with_runner(
                    repo_root.clone(),
                    state_root.clone(),
                    None,
                    true,
                    |runner| {
                        runner.mark_detach_setup_failed(&campaign.campaign_id, &error)?;
                        Ok(1)
                    },
                );
                Err(error)
            }
        };
    }
    let mut runner_args = vec![campaign.campaign_id.clone()];
    runner_args.extend(seams.to_args());
    runner_command(repo_root, state_root, &runner_args)
}

fn runner_command(
    repo_root: PathBuf,
    state_root: PathBuf,
    arguments: &[String],
) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let seams = test_seams_from_arguments(arguments)?;
    with_runner(
        repo_root,
        state_root,
        seams.fixture.as_deref(),
        seams.skip_live,
        |runner| {
            let state = runner.run(campaign_id)?;
            println!("campaign_id: {}", state.campaign_id);
            println!("state: {:?}", state.state);
            Ok(match state.state {
                rack_ai_application::CampaignState::Completed => 0,
                _ => 1,
            })
        },
    )
}

fn resume_command(
    repo_root: PathBuf,
    state_root: PathBuf,
    arguments: &[String],
) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let seams = test_seams_from_arguments(arguments)?;
    with_runner(
        repo_root,
        state_root,
        seams.fixture.as_deref(),
        seams.skip_live,
        |runner| {
            let state = runner.resume(campaign_id)?;
            println!("campaign_id: {}", state.campaign_id);
            println!("state: {:?}", state.state);
            Ok(match state.state {
                rack_ai_application::CampaignState::Completed => 0,
                _ => 1,
            })
        },
    )
}

fn supervise_command(
    repo_root: PathBuf,
    state_root: PathBuf,
    arguments: &[String],
) -> Result<i32, String> {
    let emit_json = arguments.iter().any(|value| value == "--emit-json");
    let loop_mode = arguments.iter().any(|value| value == "--loop");
    let seams = test_seams_from_arguments(arguments)?;
    let operations = load_operations_config(&repo_root)?;
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root.clone()));
    let workspace_root = registry.workspace_root()?.as_path().to_path_buf();
    let clock = SystemUnixClock;
    with_runner(
        repo_root,
        state_root.clone(),
        seams.fixture.as_deref(),
        seams.skip_live,
        |runner| loop {
            let supervisor = CampaignSupervisor::new(CampaignSupervisorDependencies {
                runner,
                clock: &clock,
                state_root: state_root.clone(),
                workspace_root: workspace_root.clone(),
                operations: operations.clone(),
            })?;
            let report = supervisor.run_once()?;
            print_supervision_report(&report, emit_json)?;
            if !loop_mode {
                return Ok(0);
            }
            thread::sleep(Duration::from_secs(
                operations.supervisor.scan_interval_seconds,
            ));
        },
    )
}

fn control_command(
    repo_root: PathBuf,
    state_root: PathBuf,
    arguments: &[String],
    control: Control,
) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    with_runner(repo_root, state_root, None, true, |runner| {
        match control {
            Control::Pause => {
                runner.pause(campaign_id)?;
                println!("campaign_id: {campaign_id}");
                println!("status: pause_requested");
            }
        }
        Ok(0)
    })
}

fn cancel_command(
    repo_root: PathBuf,
    state_root: PathBuf,
    arguments: &[String],
) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let reason = flag_value(arguments, "--reason");
    with_runner(repo_root, state_root, None, true, |runner| {
        runner.cancel(campaign_id, reason.as_deref())?;
        println!("campaign_id: {campaign_id}");
        println!("status: cancelled");
        Ok(0)
    })
}

fn revise_command(
    repo_root: PathBuf,
    state_root: PathBuf,
    arguments: &[String],
) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let revision_path = arguments.get(1).ok_or("expected revision path")?;
    let revision = load_revision_file(revision_path)?;
    with_runner(repo_root, state_root, None, true, |runner| {
        runner.revise(campaign_id, revision)?;
        println!("campaign_id: {campaign_id}");
        println!("status: revised");
        Ok(0)
    })
}

fn status_command(state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let emit_json = arguments.iter().any(|value| value == "--emit-json");
    let path = state_root
        .join("state")
        .join("campaigns")
        .join(campaign_id)
        .join("state.json");
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    if emit_json {
        print!("{content}");
        return Ok(0);
    }
    let state: CampaignStatus =
        serde_json::from_str(&content).map_err(|error| error.to_string())?;
    println!("campaign_id: {}", state.campaign_id);
    println!("state: {:?}", state.state);
    println!("branch: {}", state.branch);
    println!("worktree: {}", state.worktree_path);
    println!("head_sha: {}", state.current_head_sha);
    if let Some(worker) = state.current_worker {
        println!("current_worker: {worker}");
    }
    if let Some(action) = state.current_action {
        println!("current_action: {action}");
    }
    if let Some(step) = state.current_step_id {
        println!("current_step: {step}");
    }
    println!("attempt: {}", state.current_attempt);
    println!("elapsed_seconds: {}", state.duration_seconds);
    println!("remaining_seconds: {}", state.remaining_seconds);
    println!("last_heartbeat: {}", state.last_heartbeat);
    println!(
        "packet_root: {}",
        state_root
            .join("state")
            .join("campaigns")
            .join(campaign_id)
            .display()
    );
    if let Some(error) = state.error_message {
        println!("last_error: {error}");
    }
    if let Some(reason) = state.blocked_reason {
        println!("blocked_reason: {reason}");
    }
    Ok(0)
}

fn events_command(state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let follow = arguments.iter().any(|value| value == "--follow");
    let emit_json = arguments.iter().any(|value| value == "--emit-json");
    let path = state_root
        .join("state")
        .join("campaigns")
        .join(campaign_id)
        .join("events.jsonl");
    if !follow {
        let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        print!("{}", format_events(&content, emit_json)?);
        return Ok(0);
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(|error| error.to_string())?;
    print!("{}", format_events(&buf, emit_json)?);
    loop {
        thread::sleep(Duration::from_millis(200));
        let mut extra = String::new();
        file.read_to_string(&mut extra)
            .map_err(|error| error.to_string())?;
        if extra.is_empty() {
            let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
            if metadata.len() < buf.len() as u64 {
                file.seek(SeekFrom::Start(0))
                    .map_err(|error| error.to_string())?;
            }
            continue;
        }
        print!("{}", format_events(&extra, emit_json)?);
        buf.push_str(&extra);
    }
}

fn format_events(content: &str, emit_json: bool) -> Result<String, String> {
    if emit_json {
        return Ok(content.to_string());
    }
    let mut output = String::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let event: CampaignEvent = serde_json::from_str(line).map_err(|error| error.to_string())?;
        output.push_str(&format_event_human(&event));
        output.push('\n');
    }
    Ok(output)
}

fn format_event_human(event: &CampaignEvent) -> String {
    let mut line = format!("{} {}", event.timestamp, event.event_type);
    if let Some(step_id) = &event.step_id {
        line.push_str(&format!(" step={step_id}"));
    }
    if let Some(attempt) = event.attempt {
        line.push_str(&format!(" attempt={attempt}"));
    }
    if let Some(worker_id) = &event.worker_id {
        line.push_str(&format!(" worker={worker_id}"));
    }
    line.push_str(&format!(": {}", event.message));
    line
}

fn inspect_command(state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let campaign_id = arguments.first().ok_or("expected campaign id")?;
    let step_id = flag_value(arguments, "--step");
    let root = state_root.join("state").join("campaigns").join(campaign_id);
    let state: CampaignStatus = serde_json::from_str(
        &fs::read_to_string(root.join("state.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let step = if let Some(step_id) = step_id {
        state
            .steps
            .iter()
            .find(|item| item.step_id == step_id)
            .ok_or_else(|| format!("unknown step {step_id}"))?
    } else {
        state.steps.last().ok_or("campaign has no steps")?
    };
    println!("campaign_id: {}", state.campaign_id);
    println!("step_id: {}", step.step_id);
    println!("disposition: {}", step.disposition);
    if let Some(review) = &step.review_disposition {
        println!("review_disposition: {review:?}");
    }
    if let Some(rationale) = &step.review_rationale {
        println!("review_rationale: {rationale}");
    }
    if let Some(commit) = &step.accepted_commit {
        println!("accepted_commit: {commit}");
    }
    if let Some(attempt) = step.attempts.last() {
        println!("worker_id: {}", attempt.worker_id);
        println!("attempt: {}", attempt.attempt);
        println!("classification: {:?}", attempt.classification);
        let dir = root
            .join("steps")
            .join(&step.step_id)
            .join(format!("attempt-{}", attempt.attempt));
        for name in [
            "review-packet.json",
            "worker-transcript.json",
            "command-evidence.json",
            "git-evidence.json",
        ] {
            let path = dir.join(name);
            if path.exists() {
                println!("--- {name} ---");
                print!(
                    "{}",
                    fs::read_to_string(path).map_err(|error| error.to_string())?
                );
            }
        }
    }
    Ok(0)
}

fn ensure_detach_preflight() -> Result<(), String> {
    let systemd = Command::new("systemd-run")
        .arg("--help")
        .output()
        .map_err(|_| missing_systemd_message())?;
    if !systemd.status.success() {
        return Err(missing_systemd_message());
    }
    let user = Command::new("systemctl")
        .args(["--user", "is-system-running"])
        .output()
        .map_err(|_| missing_systemd_message())?;
    if !user.status.success() {
        return Err(missing_systemd_message());
    }
    Ok(())
}

fn spawn_detached_runner(
    repo_root: &Path,
    state_root: &Path,
    campaign_id: &str,
) -> Result<(), String> {
    ensure_detach_preflight()?;
    let unit = format!("rack-ai-campaign-{campaign_id}");
    let wrapper = repo_root.join("bin/rack-campaign");
    let status = Command::new("systemd-run")
        .args([
            "--user",
            "--collect",
            "--unit",
            unit.as_str(),
            "--working-directory",
            repo_root.to_string_lossy().as_ref(),
            wrapper.to_string_lossy().as_ref(),
            "runner",
            campaign_id,
            "--repo-root",
            repo_root.to_string_lossy().as_ref(),
            "--state-root",
            state_root.to_string_lossy().as_ref(),
        ])
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!(
            "systemd-run --user failed for unit {unit}; {}",
            missing_systemd_message()
        ));
    }
    Ok(())
}

#[cfg(test)]
fn create_campaign_after_detach_preflight(
    detach: bool,
    preflight: impl FnOnce() -> Result<(), String>,
    create: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if detach {
        preflight()?;
    }
    create()
}

fn missing_systemd_message() -> String {
    "user-level systemd is required for --detach. Enable lingering with `loginctl enable-linger $USER`, then confirm `systemctl --user is-system-running` works over SSH. Detached campaigns must not fall back to nohup or background shells.".to_string()
}

fn with_runner<F>(
    repo_root: PathBuf,
    state_root: PathBuf,
    fixture: Option<&str>,
    skip_live: bool,
    body: F,
) -> Result<i32, String>
where
    F: FnOnce(&CampaignRunner<'_>) -> Result<i32, String>,
{
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root.clone()));
    let policy = registry.command_policy()?;
    let operations = load_operations_config(&repo_root)?;
    let git = GitCommandWorktree;
    let executor_config = registry.executor_config()?;
    let container_tracker = Arc::new(CampaignContainerTracker::new(state_root.clone()));
    let executor = ConfiguredWorkspaceExecutor::with_observer(
        executor_config.clone(),
        container_tracker.clone(),
    )?;
    let live_implementer = JCodeChangeImplementer::new(RegistryPaths::new(repo_root.clone()), None);
    let fixture_document = load_fixture_document(fixture)?;
    let scripted = fixture_document
        .as_ref()
        .map(|document| ScriptedChangeImplementer::from_document(&executor, document.clone()));
    let implementer: &dyn ChangeImplementer = if let Some(scripted) = scripted.as_ref() {
        scripted
    } else {
        &live_implementer
    };
    let workers = RegistryWorkers {
        repo_root: repo_root.clone(),
    };
    let live_health = LiveHealth {
        repo_root: repo_root.clone(),
    };
    let permissive = PermissiveHealth;
    let health: &dyn CampaignHealth = if skip_live { &permissive } else { &live_health };
    let clock = SystemUnixClock;
    let sleeper = SystemRecoverySleeper;
    let reviewer = LocalPrimaryReviewer::local_default();
    let recovery_reasoner = LocalPrimaryRecoveryReasoner::local_default();

    let runner = CampaignRunner::new(CampaignRunnerDependencies {
        registry: &registry,
        command_policy: &policy,
        git: &git,
        implementer,
        executor: &executor,
        workers: &workers,
        health,
        clock: &clock,
        sleeper: &sleeper,
        worker_recovery_max_wait_seconds: operations.supervisor.worker_recovery_max_wait_seconds,
        worker_recovery_retry_delays_seconds: operations
            .supervisor
            .worker_recovery_retry_delays_seconds
            .clone(),
        worker_recovery_max_attempts: operations.supervisor.worker_recovery_max_attempts,
        state_root,
        container_tracker: Some(container_tracker),
    });

    let runner = if fixture_document.is_none() && !skip_live {
        runner
            .with_reviewer(&reviewer)
            .with_recovery_reasoner(&recovery_reasoner)
    } else {
        runner
    };

    body(&runner)
}

fn load_campaign_file(path: &str) -> Result<Campaign, String> {
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

fn load_operations_config(repo_root: &Path) -> Result<OperationsConfig, String> {
    let path = repo_root.join("config").join("operations.json");
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    let config = serde_json::from_str::<OperationsConfig>(&content)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))?;
    config.validate()?;
    Ok(config)
}

fn print_supervision_report(
    report: &CampaignSupervisionReport,
    emit_json: bool,
) -> Result<(), String> {
    if emit_json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    println!("scanned_campaigns: {}", report.scanned_campaigns);
    println!("resumed_campaigns: {}", report.resumed_campaigns);
    for action in &report.actions {
        println!(
            "campaign={} action={} previous={:?} outcome={:?} message={}",
            action.campaign_id,
            action.action,
            action.previous_state,
            action.outcome_state,
            action.message
        );
    }
    for cleanup in &report.cleanup {
        println!(
            "cleanup campaign={} action={} message={}",
            cleanup.campaign_id, cleanup.action, cleanup.message
        );
    }
    Ok(())
}

fn load_revision_file(path: &str) -> Result<CampaignRevisionDocument, String> {
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

fn flag_value(arguments: &[String], flag: &str) -> Option<String> {
    arguments
        .iter()
        .position(|value| value == flag)
        .and_then(|index| arguments.get(index + 1))
        .cloned()
}

#[derive(Debug)]
struct TestSeams {
    skip_live: bool,
    fixture: Option<String>,
}

impl TestSeams {
    fn to_args(&self) -> Vec<String> {
        let mut extra = Vec::new();
        if let Some(path) = &self.fixture {
            extra.push("--fixture-implementer".to_string());
            extra.push(path.clone());
        }
        if self.skip_live {
            extra.push("--skip-live-health".to_string());
        }
        extra
    }
}

fn test_seams_from_arguments(arguments: &[String]) -> Result<TestSeams, String> {
    let requested_skip = arguments.iter().any(|value| value == "--skip-live-health");
    let requested_fixture = flag_value(arguments, "--fixture-implementer");
    if (requested_skip || requested_fixture.is_some()) && !cfg!(feature = "campaign-test-seams") {
        let flag = if requested_skip {
            "--skip-live-health"
        } else {
            "--fixture-implementer"
        };
        return Err(format!(
            "unsupported campaign flag: {flag} (test-only; rebuild with --features campaign-test-seams)"
        ));
    }
    Ok(TestSeams {
        skip_live: requested_skip && cfg!(feature = "campaign-test-seams"),
        fixture: if cfg!(feature = "campaign-test-seams") {
            requested_fixture
        } else {
            None
        },
    })
}

fn load_fixture_document(
    fixture: Option<&str>,
) -> Result<Option<ScriptedImplementerDocument>, String> {
    let Some(path) = fixture else {
        return Ok(None);
    };
    if !cfg!(feature = "campaign-test-seams") {
        return Err(
            "unsupported campaign flag: --fixture-implementer (test-only; rebuild with --features campaign-test-seams)"
                .to_string(),
        );
    }
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::create_campaign_after_detach_preflight;
    use super::format_events;
    use super::test_seams_from_arguments;
    use rack_ai_application::CampaignEvent;
    use std::cell::Cell;

    #[test]
    fn detach_preflight_runs_before_campaign_creation() {
        let created = Cell::new(false);
        let error = create_campaign_after_detach_preflight(
            true,
            || Err("user-level systemd is required for --detach".to_string()),
            || {
                created.set(true);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("systemd"));
        assert!(
            !created.get(),
            "campaign state must not be created when detach preflight fails"
        );
    }

    #[test]
    fn foreground_start_skips_detach_preflight() {
        let created = Cell::new(false);
        create_campaign_after_detach_preflight(
            false,
            || Err("should not run".to_string()),
            || {
                created.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(created.get());
    }

    #[test]
    fn events_emit_json_is_opt_in() {
        let event = CampaignEvent {
            timestamp: "1".to_string(),
            campaign_id: "c1".to_string(),
            step_id: Some("add-alpha".to_string()),
            attempt: Some(1),
            worker_id: Some("local-coder".to_string()),
            action: None,
            state: None,
            event_type: "step_accepted".to_string(),
            message: "accepted step add-alpha".to_string(),
            details: serde_json::Map::new(),
        };
        let json = format!("{}\n", serde_json::to_string(&event).unwrap());
        let as_json = format_events(&json, true).unwrap();
        assert_eq!(as_json, json);
        let human = format_events(&json, false).unwrap();
        assert!(human.contains("step_accepted"));
        assert!(human.contains("step=add-alpha"));
        assert!(human.contains("accepted step add-alpha"));
        assert!(!human.trim_start().starts_with('{'));
    }

    #[test]
    fn operator_cli_rejects_test_only_bypass_flags() {
        if cfg!(feature = "campaign-test-seams") {
            let seams = test_seams_from_arguments(&[
                "--skip-live-health".to_string(),
                "--fixture-implementer".to_string(),
                "script.json".to_string(),
            ])
            .unwrap();
            assert!(seams.skip_live);
            assert_eq!(seams.fixture.as_deref(), Some("script.json"));
            return;
        }
        let skip = test_seams_from_arguments(&["--skip-live-health".to_string()]).unwrap_err();
        assert!(skip.contains("--skip-live-health"));
        let fixture = test_seams_from_arguments(&[
            "--fixture-implementer".to_string(),
            "script.json".to_string(),
        ])
        .unwrap_err();
        assert!(fixture.contains("--fixture-implementer"));
    }
}
