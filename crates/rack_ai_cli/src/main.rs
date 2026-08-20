use std::collections::BTreeSet;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rack_ai_application::Clock;
use rack_ai_application::InspectStatus;
use rack_ai_application::InspectStatusDependencies;
use rack_ai_application::RunNextOutcome;
use rack_ai_application::RunNextTask;
use rack_ai_application::RunNextTaskDependencies;
use rack_ai_application::SubmitTask;
use rack_ai_application::SubmitTaskDependencies;
use rack_ai_application::SubmitTaskRequest;
use rack_ai_application::TaskSpec;
use rack_ai_application::WorkerCatalog;
use rack_ai_domain::AttemptLimit;
use rack_ai_domain::Placement;
use rack_ai_domain::RunStateDraft;
use rack_ai_domain::TaskId;
use rack_ai_domain::TimeoutSeconds;
use rack_ai_infrastructure::CliRackTaskExecutor;
use rack_ai_infrastructure::DirectCoderWorker;
use rack_ai_infrastructure::EndpointProbe;
use rack_ai_infrastructure::FileSystemExecutionQueueRepository;
use rack_ai_infrastructure::FileSystemLeaseRepository;
use rack_ai_infrastructure::FileSystemQueueStateRepository;
use rack_ai_infrastructure::FileSystemRegistryRepository;
use rack_ai_infrastructure::FileSystemRunStateRepository;
use rack_ai_infrastructure::FileSystemTaskSpecRepository;
use rack_ai_infrastructure::FileSystemWorkerCatalog;
use rack_ai_infrastructure::HealthcheckService;
use rack_ai_infrastructure::HealthcheckServiceDependencies;
use rack_ai_infrastructure::RegistryPaths;
use rack_ai_infrastructure::RepositoryPaths;
use rack_ai_infrastructure::UtcDateCommandClock;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

struct CommandRoots {
    repo_root: PathBuf,
    state_root: PathBuf,
}

struct NormalizedSubmitSpec {
    task_id: String,
    max_attempts: u32,
    timeout_seconds: u32,
    placement: Placement,
    dag_run_state: Option<rack_ai_domain::DagRunState>,
    spec_json: String,
}

#[derive(Deserialize)]
struct CoordinatorTemplateStep {
    name: String,
    worker: String,
    prompt: String,
}

#[derive(Deserialize)]
struct CoordinatorTemplate {
    steps: Vec<CoordinatorTemplateStep>,
}

#[derive(Clone)]
struct ArtifactExpectation {
    path: String,
    exact_text: Option<String>,
    must_contain: Option<String>,
}

#[derive(Clone)]
struct TaskStep {
    name: String,
    worker: String,
    cwd: String,
    prompt: String,
    artifacts: Vec<ArtifactExpectation>,
}

#[derive(Serialize)]
struct ArtifactCheck {
    path: String,
    exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exact_match: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contains_match: Option<bool>,
}

#[derive(Serialize)]
struct TaskStepResult {
    index: usize,
    name: String,
    worker: String,
    cwd: String,
    command: Vec<String>,
    returncode: i32,
    stdout: String,
    stderr: String,
    artifacts: Vec<ArtifactCheck>,
    started_at: String,
    finished_at: String,
    duration_seconds: f64,
    ok: bool,
    summary: String,
}

#[derive(Serialize)]
struct TaskRunResult {
    task_id: String,
    template: String,
    request: Option<String>,
    spec: String,
    step_count: usize,
    placement: Option<Value>,
    started_at: String,
    finished_at: String,
    duration_seconds: f64,
    steps: Vec<TaskStepResult>,
    ok: bool,
    summary: Vec<String>,
    log_path: String,
}

fn main() {
    match execute() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn execute() -> Result<i32, String> {
    let arguments = env::args().collect::<Vec<_>>();
    let command = arguments.get(1).ok_or("expected command")?;
    let roots = current_roots(&arguments)?;
    let paths = RepositoryPaths::new(roots.state_root.clone());
    if command == "submit" {
        let spec_path = arguments.get(2).ok_or("expected spec path")?;
        submit(
            paths,
            roots.repo_root,
            PathBuf::from(spec_path),
            optional_u32_flag(&arguments, "--max-attempts")?,
            optional_u32_flag(&arguments, "--timeout-seconds")?,
        )
    } else if command == "status" {
        status(paths, arguments.iter().any(|value| value == "--emit-json"))
    } else if command == "run-next" {
        print_run_next(run_next_once(paths, roots.repo_root)?)
    } else if command == "runner" {
        run_runner(
            paths,
            roots.repo_root,
            arguments.iter().any(|value| value == "--once"),
        )
    } else if command == "task" {
        run_task_from_arguments(roots.repo_root, &arguments[2..])
    } else if command == "coordinator" {
        run_coordinator_command(roots.repo_root, &arguments[2..])
    } else if command == "coder-worker" {
        run_coder_worker(&arguments[2..])
    } else if command == "healthcheck" {
        healthcheck(roots.repo_root)
    } else {
        Err("unsupported command".to_string())
    }
}

fn current_roots(arguments: &[String]) -> Result<CommandRoots, String> {
    let working_directory = env::current_dir().map_err(|error| error.to_string())?;
    let repo_root = if let Some(value) = flag_value(arguments, "--repo-root") {
        PathBuf::from(value)
    } else if let Some(value) = flag_value(arguments, "--root") {
        PathBuf::from(value)
    } else {
        working_directory.clone()
    };
    let state_root = if let Some(value) = flag_value(arguments, "--state-root") {
        PathBuf::from(value)
    } else if flag_value(arguments, "--repo-root").is_some() {
        repo_root.clone()
    } else if let Some(value) = flag_value(arguments, "--root") {
        PathBuf::from(value)
    } else {
        repo_root.clone()
    };
    Ok(CommandRoots {
        repo_root,
        state_root,
    })
}

fn flag_value(arguments: &[String], flag: &str) -> Option<String> {
    arguments
        .iter()
        .position(|value| value == flag)
        .and_then(|index| arguments.get(index + 1))
        .cloned()
}

fn optional_u32_flag(arguments: &[String], flag: &str) -> Result<Option<u32>, String> {
    match flag_value(arguments, flag) {
        Some(value) => value
            .parse::<u32>()
            .map(Some)
            .map_err(|error| error.to_string()),
        None => Ok(None),
    }
}

fn submit(
    paths: RepositoryPaths,
    repo_root: PathBuf,
    spec_path: PathBuf,
    max_attempts_override: Option<u32>,
    timeout_seconds_override: Option<u32>,
) -> Result<i32, String> {
    let spec_json = fs::read_to_string(&spec_path).map_err(|error| error.to_string())?;
    let clock = UtcDateCommandClock;
    let worker_catalog = FileSystemWorkerCatalog::new(RegistryPaths::new(repo_root));
    let normalized = normalize_submit_spec(
        &spec_json,
        &worker_catalog,
        max_attempts_override,
        timeout_seconds_override,
    )?;
    let run_state_repository = FileSystemRunStateRepository::new(paths.clone());
    let task_spec_repository = FileSystemTaskSpecRepository::new(paths.clone());
    let service = SubmitTask::new(SubmitTaskDependencies {
        run_state_repository: &run_state_repository,
        task_spec_repository: &task_spec_repository,
    });
    let task_id = TaskId::new(normalized.task_id.clone())?;
    let request = SubmitTaskRequest {
        spec_json: normalized.spec_json,
        run_state: RunStateDraft {
            task_id: task_id.clone(),
            attempt_limit: AttemptLimit::new(normalized.max_attempts)?,
            timeout_seconds: TimeoutSeconds::new(normalized.timeout_seconds)?,
            placement: normalized.placement,
        },
        dag_run_state: normalized.dag_run_state,
        submitted_at: clock.now_text()?,
        source_spec: spec_path.to_string_lossy().to_string(),
        queue_path: paths
            .queued_dir()
            .join(format!("{}.json", task_id.value()))
            .to_string_lossy()
            .to_string(),
    };
    let run_state = service.execute(request)?;
    println!("{}", run_state.task_id().value());
    Ok(0)
}

fn status(paths: RepositoryPaths, emit_json: bool) -> Result<i32, String> {
    let queue_state_repository = FileSystemQueueStateRepository::new(paths.clone());
    let lease_state_repository = FileSystemLeaseRepository::new(paths.clone());
    let run_state_repository = FileSystemRunStateRepository::new(paths);
    let service = InspectStatus::new(InspectStatusDependencies {
        queue_state_repository: &queue_state_repository,
        lease_state_repository: &lease_state_repository,
        run_state_repository: &run_state_repository,
    });
    let snapshot = service.execute()?;
    if emit_json {
        let json = serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?;
        println!("{json}");
        return Ok(0);
    }

    println!("Queued: {}", snapshot.queued().len());
    for item in snapshot.queued() {
        println!("  - {item}");
    }
    println!("Running: {}", snapshot.running().len());
    for item in snapshot.running() {
        println!("  - {item}");
    }
    println!("Leases: {}", snapshot.leases().len());
    for lease in snapshot.leases() {
        let task_id = lease.task_id().cloned().unwrap_or_default();
        println!("  - {}: {}", lease.resource_id(), task_id);
    }
    println!("Runs: {}", snapshot.runs().len());
    for item in snapshot.runs() {
        let waiting = item.waiting_on_resources();
        let waiting_text = if waiting.is_empty() {
            String::new()
        } else {
            format!(" waiting={}", waiting.join(","))
        };
        let active_node = item
            .active_node_id()
            .map(|value| format!(" node={value}"))
            .unwrap_or_default();
        let admission = item
            .admission_state()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "  - {}: {} (attempt {}/{}, admission={}){}{}",
            item.task_id(),
            item.status(),
            item.attempt(),
            item.max_attempts(),
            admission,
            active_node,
            waiting_text
        );
    }
    Ok(0)
}

fn run_next_once(paths: RepositoryPaths, repo_root: PathBuf) -> Result<RunNextOutcome, String> {
    let state_root = paths.root().to_path_buf();
    let clock = UtcDateCommandClock;
    let execution_queue_repository = FileSystemExecutionQueueRepository::new(paths.clone());
    let lease_repository = FileSystemLeaseRepository::new(paths.clone());
    let run_state_repository = FileSystemRunStateRepository::new(paths.clone());
    let task_spec_repository = FileSystemTaskSpecRepository::new(paths);
    let worker_catalog = FileSystemWorkerCatalog::new(RegistryPaths::new(repo_root.clone()));
    let task_executor = CliRackTaskExecutor::new(repo_root, state_root);
    let service = RunNextTask::new(RunNextTaskDependencies {
        clock: &clock,
        execution_queue_repository: &execution_queue_repository,
        lease_repository: &lease_repository,
        run_state_repository: &run_state_repository,
        task_executor: &task_executor,
        task_spec_repository: &task_spec_repository,
        worker_catalog: &worker_catalog,
    });
    service.execute()
}

fn run_runner(paths: RepositoryPaths, repo_root: PathBuf, once: bool) -> Result<i32, String> {
    if once {
        return print_run_next(run_next_once(paths, repo_root)?);
    }
    loop {
        let outcome = run_next_once(paths.clone(), repo_root.clone())?;
        let stop = matches!(outcome, RunNextOutcome::NoQueuedTasks);
        print_run_next(outcome)?;
        if stop {
            return Ok(0);
        }
    }
}

fn print_run_next(outcome: RunNextOutcome) -> Result<i32, String> {
    match outcome {
        RunNextOutcome::NoQueuedTasks => println!("No queued tasks."),
        RunNextOutcome::NoAdmissibleTasks => println!("No admissible queued tasks."),
        RunNextOutcome::Succeeded(task_id) => println!("{task_id}"),
        RunNextOutcome::Requeued(task_id) => println!("Requeued {task_id}"),
        RunNextOutcome::Failed(task_id) => println!("Failed {task_id}"),
    }
    Ok(0)
}

fn run_task_from_arguments(repo_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let mut emit_json = false;
    let mut spec_path: Option<PathBuf> = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--emit-json" => {
                emit_json = true;
                index += 1;
            }
            "--repo-root" | "--state-root" | "--root" => {
                index += 2;
            }
            value if value.starts_with("--") => {
                return Err(format!("unknown argument: {value}"));
            }
            value => {
                if spec_path.is_some() {
                    return Err("expected a single spec path".to_string());
                }
                spec_path = Some(PathBuf::from(value));
                index += 1;
            }
        }
    }
    let spec_path = spec_path.ok_or("expected spec path")?;
    run_task_command(repo_root, spec_path, emit_json)
}

fn run_coder_worker(arguments: &[String]) -> Result<i32, String> {
    let mut cwd = ".".to_string();
    let mut prompt_file: Option<String> = None;
    let mut max_turns = 6_usize;
    let mut prompt: Option<String> = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--cwd" => {
                cwd = arguments.get(index + 1).ok_or("missing cwd value")?.clone();
                index += 2;
            }
            "--prompt-file" => {
                prompt_file = Some(
                    arguments
                        .get(index + 1)
                        .ok_or("missing prompt-file value")?
                        .clone(),
                );
                index += 2;
            }
            "--max-turns" => {
                max_turns = arguments
                    .get(index + 1)
                    .ok_or("missing max-turns value")?
                    .parse::<usize>()
                    .map_err(|error| error.to_string())?;
                index += 2;
            }
            "--repo-root" | "--state-root" | "--root" => {
                index += 2;
            }
            "--" => {
                prompt = Some(arguments[index + 1..].join(" "));
                break;
            }
            value if value.starts_with("--") => return Err(format!("unknown argument: {value}")),
            _ => {
                prompt = Some(arguments[index..].join(" "));
                break;
            }
        }
    }

    let prompt_text = if let Some(path) = prompt_file {
        fs::read_to_string(path).map_err(|error| error.to_string())?
    } else if let Some(value) = prompt {
        value
    } else {
        read_stdin_text()?
    };
    if prompt_text.trim().is_empty() {
        return Err("Prompt is empty.".to_string());
    }

    let workdir = PathBuf::from(cwd);
    fs::create_dir_all(&workdir).map_err(|error| error.to_string())?;
    let worker = DirectCoderWorker::local_default();
    let final_text = worker.execute(&prompt_text, &workdir, max_turns)?;
    println!("{final_text}");
    Ok(if final_text.trim() == "COMPLETE" {
        0
    } else {
        1
    })
}

fn run_task_command(
    repo_root: PathBuf,
    spec_path: PathBuf,
    emit_json: bool,
) -> Result<i32, String> {
    let spec = load_json_object(&spec_path)?;
    let steps = build_task_steps(&spec)?;
    let started = now_system_time();
    let task_id = spec
        .get("task_id")
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .unwrap_or_else(generated_task_id);
    let request = spec
        .get("request")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let template = spec
        .get("template")
        .and_then(Value::as_str)
        .unwrap_or("legacy")
        .to_string();
    let placement = spec.get("placement").cloned();
    let mut step_results = Vec::new();
    let mut overall_ok = true;
    for (index, step) in steps.iter().enumerate() {
        let result = execute_task_step(&repo_root, step, index + 1)?;
        if !result.ok {
            overall_ok = false;
            step_results.push(result);
            break;
        }
        step_results.push(result);
    }
    let finished = now_system_time();
    let summary = step_results
        .iter()
        .map(|step| step.summary.clone())
        .collect::<Vec<_>>();
    let mut final_result = TaskRunResult {
        task_id,
        template,
        request,
        spec: spec_path.to_string_lossy().to_string(),
        step_count: steps.len(),
        placement,
        started_at: iso_z(started),
        finished_at: iso_z(finished),
        duration_seconds: duration_seconds(started, finished),
        steps: step_results,
        ok: overall_ok,
        summary,
        log_path: String::new(),
    };
    final_result.log_path = write_task_run_log(&repo_root, &final_result)?;

    if emit_json {
        let json =
            serde_json::to_string_pretty(&final_result).map_err(|error| error.to_string())?;
        println!("{json}");
    } else {
        for result in &final_result.steps {
            println!("== {} ({}) ==", result.name, result.worker);
            print!("{}", result.stdout);
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
            println!("\nArtifact checks:");
            for artifact in &result.artifacts {
                let json =
                    serde_json::to_string_pretty(artifact).map_err(|error| error.to_string())?;
                println!("{json}");
            }
            println!("Summary: {}", result.summary);
        }
        println!("Run log: {}", final_result.log_path);
    }

    Ok(if final_result.ok { 0 } else { 1 })
}

fn run_coordinator_command(repo_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let templates = load_templates(&repo_root)?;
    let mut template_name: Option<String> = None;
    let mut auto_template = false;
    let mut cwd = "/tmp/jcode-rack-test".to_string();
    let mut request_file: Option<String> = None;
    let mut output: Option<String> = None;
    let mut artifact_exact = Vec::new();
    let mut artifact_contains = Vec::new();
    let mut preview = false;
    let mut run = false;
    let mut request: Option<String> = None;

    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--template" => {
                template_name = Some(
                    arguments
                        .get(index + 1)
                        .ok_or("missing template value")?
                        .clone(),
                );
                index += 2;
            }
            "--auto-template" => {
                auto_template = true;
                index += 1;
            }
            "--cwd" => {
                cwd = arguments.get(index + 1).ok_or("missing cwd value")?.clone();
                index += 2;
            }
            "--request-file" => {
                request_file = Some(
                    arguments
                        .get(index + 1)
                        .ok_or("missing request-file value")?
                        .clone(),
                );
                index += 2;
            }
            "--output" => {
                output = Some(
                    arguments
                        .get(index + 1)
                        .ok_or("missing output value")?
                        .clone(),
                );
                index += 2;
            }
            "--artifact-exact" => {
                artifact_exact.push(
                    arguments
                        .get(index + 1)
                        .ok_or("missing artifact-exact value")?
                        .clone(),
                );
                index += 2;
            }
            "--artifact-contains" => {
                artifact_contains.push(
                    arguments
                        .get(index + 1)
                        .ok_or("missing artifact-contains value")?
                        .clone(),
                );
                index += 2;
            }
            "--preview" => {
                preview = true;
                index += 1;
            }
            "--run" => {
                run = true;
                index += 1;
            }
            "--repo-root" | "--state-root" | "--root" => {
                index += 2;
            }
            value if value.starts_with("--") => return Err(format!("unknown argument: {value}")),
            _ => {
                request = Some(arguments[index..].join(" "));
                break;
            }
        }
    }

    let request_text = if let Some(path) = request_file {
        fs::read_to_string(path).map_err(|error| error.to_string())?
    } else if let Some(value) = request {
        value
    } else {
        read_stdin_text()?
    };
    if request_text.trim().is_empty() {
        return Err("Request is empty.".to_string());
    }

    let selected_template = template_name.unwrap_or_else(|| {
        if auto_template {
            choose_template(&request_text)
        } else {
            "patch".to_string()
        }
    });
    let exact_artifacts = parse_artifact_pairs(&artifact_exact, true)?;
    let contains_artifacts = parse_artifact_pairs(&artifact_contains, false)?;
    let spec = build_coordinator_spec(
        templates
            .get(&selected_template)
            .ok_or(format!("unknown template: {selected_template}"))?,
        &selected_template,
        &request_text,
        &cwd,
        exact_artifacts,
        contains_artifacts,
    )?;

    let spec_path = if let Some(path) = output {
        PathBuf::from(path)
    } else {
        let spec_dir = repo_root.join("logs/specs");
        fs::create_dir_all(&spec_dir).map_err(|error| error.to_string())?;
        spec_dir.join(format!(
            "{}.json",
            spec["task_id"].as_str().unwrap_or("task")
        ))
    };
    if let Some(parent) = spec_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let spec_json = serde_json::to_string_pretty(&spec).map_err(|error| error.to_string())?;
    fs::write(&spec_path, format!("{spec_json}\n")).map_err(|error| error.to_string())?;

    if preview {
        println!("{spec_json}");
        if !run {
            return Ok(0);
        }
    }

    if !run {
        println!("{}", spec_path.to_string_lossy());
        return Ok(0);
    }

    run_task_command(repo_root, spec_path, true)
}

fn healthcheck(repo_root: PathBuf) -> Result<i32, String> {
    let registry_repository = FileSystemRegistryRepository::new(RegistryPaths::new(repo_root));
    let probe = EndpointProbe;
    let service = HealthcheckService::new(HealthcheckServiceDependencies {
        endpoint_probe: &probe,
        registry_repository: &registry_repository,
    });
    let snapshot = service.execute()?;
    let json = serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(0)
}

fn normalize_submit_spec(
    source_json: &str,
    worker_catalog: &dyn WorkerCatalog,
    max_attempts_override: Option<u32>,
    timeout_seconds_override: Option<u32>,
) -> Result<NormalizedSubmitSpec, String> {
    let mut value =
        serde_json::from_str::<Value>(source_json).map_err(|error| error.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or("expected JSON object for submit spec".to_string())?;
    let task_id = read_required_string(object, "task_id")?;
    let max_attempts = max_attempts_override
        .or_else(|| read_optional_u32(object, "max_attempts"))
        .unwrap_or(1);
    let timeout_seconds = timeout_seconds_override
        .or_else(|| read_optional_u32(object, "timeout_seconds"))
        .unwrap_or(900);
    object.insert("max_attempts".to_string(), Value::from(max_attempts));
    object.insert("timeout_seconds".to_string(), Value::from(timeout_seconds));

    let placement = if let Some(existing) = object.get("placement") {
        serde_json::from_value::<Placement>(existing.clone()).map_err(|error| error.to_string())?
    } else {
        let placement = derive_placement_from_object(object, worker_catalog)?;
        object.insert(
            "placement".to_string(),
            serde_json::to_value(&placement).map_err(|error| error.to_string())?,
        );
        placement
    };

    let task_spec =
        serde_json::from_value::<TaskSpec>(value.clone()).map_err(|error| error.to_string())?;
    let dag_run_state = task_spec.dag_run_state()?;
    let spec_json = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    Ok(NormalizedSubmitSpec {
        task_id,
        max_attempts,
        timeout_seconds,
        placement,
        dag_run_state,
        spec_json,
    })
}

fn derive_placement_from_object(
    object: &Map<String, Value>,
    worker_catalog: &dyn WorkerCatalog,
) -> Result<Placement, String> {
    let worker_ids = extract_worker_ids(object)?;
    let mut resource_ids = Vec::new();
    let mut model_ids = Vec::new();
    let mut backends = Vec::new();
    for worker_id in &worker_ids {
        let binding = worker_catalog.resolve(worker_id)?;
        let placement = binding.placement();
        for resource_id in placement.resource_ids() {
            push_unique(&mut resource_ids, resource_id.clone());
        }
        for model_id in placement.model_ids() {
            push_unique(&mut model_ids, model_id.clone());
        }
        for backend in placement.backends() {
            push_unique(&mut backends, backend.clone());
        }
    }
    Ok(Placement::new(worker_ids, resource_ids)
        .with_models(model_ids)
        .with_backends(backends))
}

fn extract_worker_ids(object: &Map<String, Value>) -> Result<Vec<String>, String> {
    if let Some(nodes) = object
        .get("dag")
        .and_then(|value| value.get("nodes"))
        .and_then(Value::as_array)
    {
        if nodes.is_empty() {
            return Err("dag nodes must be a non-empty list".to_string());
        }
        let mut worker_ids = Vec::new();
        let mut node_ids = BTreeSet::new();
        for node in nodes {
            let node_object = node
                .as_object()
                .ok_or("dag node entries must be objects".to_string())?;
            let node_id = read_required_string(node_object, "id")?;
            if !node_ids.insert(node_id.clone()) {
                return Err(format!("duplicate dag node id: {node_id}"));
            }
            read_required_string(node_object, "cwd")?;
            read_required_string(node_object, "prompt")?;
            push_unique(
                &mut worker_ids,
                read_required_string(node_object, "worker")?,
            );
        }
        for node in nodes {
            let node_object = node
                .as_object()
                .ok_or("dag node entries must be objects".to_string())?;
            let node_id = read_required_string(node_object, "id")?;
            if let Some(depends_on) = node_object.get("depends_on").and_then(Value::as_array) {
                for dependency in depends_on {
                    let dependency_id = dependency
                        .as_str()
                        .ok_or(format!("dag node {node_id} depends on non-string node id"))?;
                    if !node_ids.contains(dependency_id) {
                        return Err(format!(
                            "dag node {node_id} depends on unknown node {dependency_id}"
                        ));
                    }
                }
            }
        }
        return Ok(worker_ids);
    }

    if let Some(steps) = object.get("steps").and_then(Value::as_array) {
        let mut worker_ids = Vec::new();
        for step in steps {
            let step_object = step
                .as_object()
                .ok_or("step entries must be objects".to_string())?;
            if let Some(worker) = step_object.get("worker").and_then(Value::as_str) {
                push_unique(&mut worker_ids, worker.to_string());
            }
        }
        if !worker_ids.is_empty() {
            return Ok(worker_ids);
        }
    }

    if let Some(worker) = object.get("worker").and_then(Value::as_str) {
        return Ok(vec![worker.to_string()]);
    }

    Err("task spec does not declare any workers".to_string())
}

fn load_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str::<Value>(&content)
        .map_err(|error| error.to_string())?
        .as_object()
        .cloned()
        .ok_or("expected JSON object".to_string())
}

fn build_task_steps(spec: &Map<String, Value>) -> Result<Vec<TaskStep>, String> {
    if let Some(steps) = spec.get("steps").and_then(Value::as_array) {
        if steps.is_empty() {
            return Err("steps must be a non-empty list".to_string());
        }
        return steps
            .iter()
            .enumerate()
            .map(|(index, value)| build_task_step(value, &format!("step-{}", index + 1)))
            .collect();
    }

    let worker = read_required_string(spec, "worker")?;
    let cwd = read_required_string(spec, "cwd")?;
    let prompt = read_required_string(spec, "prompt")?;
    let artifacts = parse_artifacts(spec.get("artifacts"))?;
    Ok(vec![TaskStep {
        name: spec
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("single-step")
            .to_string(),
        worker,
        cwd,
        prompt,
        artifacts,
    }])
}

fn build_task_step(value: &Value, default_name: &str) -> Result<TaskStep, String> {
    let object = value
        .as_object()
        .ok_or("step entries must be objects".to_string())?;
    Ok(TaskStep {
        name: object
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(default_name)
            .to_string(),
        worker: read_required_string(object, "worker")?,
        cwd: read_required_string(object, "cwd")?,
        prompt: read_required_string(object, "prompt")?,
        artifacts: parse_artifacts(object.get("artifacts"))?,
    })
}

fn parse_artifacts(value: Option<&Value>) -> Result<Vec<ArtifactExpectation>, String> {
    let Some(array) = value else {
        return Ok(vec![]);
    };
    let items = array
        .as_array()
        .ok_or("artifacts must be a list".to_string())?;
    items
        .iter()
        .map(|item| {
            let object = item
                .as_object()
                .ok_or("artifact entries must be objects".to_string())?;
            Ok(ArtifactExpectation {
                path: read_required_string(object, "path")?,
                exact_text: object
                    .get("exact_text")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
                must_contain: object
                    .get("must_contain")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
            })
        })
        .collect()
}

fn execute_task_step(
    repo_root: &Path,
    step: &TaskStep,
    index: usize,
) -> Result<TaskStepResult, String> {
    let worker_entrypoint = resolve_worker_entrypoint(repo_root, &step.worker)?;
    let started = now_system_time();
    let output = Command::new(&worker_entrypoint)
        .arg("--cwd")
        .arg(&step.cwd)
        .arg("--")
        .arg(&step.prompt)
        .output()
        .map_err(|error| error.to_string())?;
    let finished = now_system_time();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let artifact_results = validate_artifacts(&step.artifacts)?;
    let artifact_failures = artifact_results.iter().any(|item| {
        !item.exists || item.exact_match == Some(false) || item.contains_match == Some(false)
    });
    let ok = output.status.success() && !artifact_failures;
    let summary = summarize_step(&step.name, &step.worker, ok, &artifact_results);
    let returncode = output.status.code().unwrap_or(1);
    Ok(TaskStepResult {
        index,
        name: step.name.clone(),
        worker: step.worker.clone(),
        cwd: step.cwd.clone(),
        command: vec![
            worker_entrypoint.to_string_lossy().to_string(),
            "--cwd".to_string(),
            step.cwd.clone(),
            "--".to_string(),
            step.prompt.clone(),
        ],
        returncode,
        stdout,
        stderr,
        artifacts: artifact_results,
        started_at: iso_z(started),
        finished_at: iso_z(finished),
        duration_seconds: duration_seconds(started, finished),
        ok,
        summary,
    })
}

fn resolve_worker_entrypoint(repo_root: &Path, worker_id: &str) -> Result<PathBuf, String> {
    let repository = FileSystemRegistryRepository::new(RegistryPaths::new(repo_root.to_path_buf()));
    let worker = repository
        .load_workers()?
        .into_iter()
        .find(|item| item.id == worker_id)
        .ok_or(format!("unsupported worker: {worker_id}"))?;
    Ok(PathBuf::from(worker.entrypoint))
}

fn validate_artifacts(artifacts: &[ArtifactExpectation]) -> Result<Vec<ArtifactCheck>, String> {
    let mut results = Vec::new();
    for artifact in artifacts {
        let path = PathBuf::from(&artifact.path);
        let exists = path.exists();
        let mut entry = ArtifactCheck {
            path: artifact.path.clone(),
            exists,
            size: None,
            exact_match: None,
            contains_match: None,
        };
        if exists {
            let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            entry.size = Some(content.len());
            if let Some(expected) = &artifact.exact_text {
                entry.exact_match = Some(content == *expected);
            }
            if let Some(expected) = &artifact.must_contain {
                entry.contains_match = Some(content.contains(expected));
            }
        }
        results.push(entry);
    }
    Ok(results)
}

fn summarize_step(name: &str, worker: &str, ok: bool, artifacts: &[ArtifactCheck]) -> String {
    if !ok {
        return format!("{name} failed on {worker}");
    }
    if artifacts.is_empty() {
        return format!("{name} passed");
    }
    let ok_count = artifacts.iter().filter(|item| item.exists).count();
    format!("{name} passed with {ok_count} artifact checks")
}

fn write_task_run_log(repo_root: &Path, result: &TaskRunResult) -> Result<String, String> {
    let log_dir = repo_root.join("logs/runs");
    fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
    let mut path = log_dir.join(format!("{}.json", result.task_id));
    let mut suffix = 1;
    while path.exists() {
        path = log_dir.join(format!("{}-{}.json", result.task_id, suffix));
        suffix += 1;
    }
    let json = serde_json::to_string_pretty(result).map_err(|error| error.to_string())?;
    fs::write(&path, format!("{json}\n")).map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

fn load_templates(repo_root: &Path) -> Result<HashMap<String, CoordinatorTemplate>, String> {
    let path = repo_root.join("config/task_templates.json");
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str::<HashMap<String, CoordinatorTemplate>>(&content)
        .map_err(|error| error.to_string())
}

fn parse_artifact_pairs(
    values: &[String],
    exact: bool,
) -> Result<Vec<ArtifactExpectation>, String> {
    values
        .iter()
        .map(|value| {
            let mut parts = value.splitn(2, '=');
            let path = parts.next().unwrap_or_default();
            let marker = parts
                .next()
                .ok_or("artifact format must be path=value".to_string())?;
            Ok(ArtifactExpectation {
                path: path.to_string(),
                exact_text: if exact {
                    Some(marker.to_string())
                } else {
                    None
                },
                must_contain: if exact {
                    None
                } else {
                    Some(marker.to_string())
                },
            })
        })
        .collect()
}

fn choose_template(request: &str) -> String {
    let text = request.to_lowercase();
    let padded = format!(" {text} ");
    let has_phrase = |phrases: &[&str]| {
        phrases
            .iter()
            .any(|phrase| padded.contains(&format!(" {phrase} ")))
    };
    if has_phrase(&["verify tests", "run tests", "smoke test"]) {
        return "test".to_string();
    }
    if text
        .split_whitespace()
        .any(|token| ["test", "pytest", "unittest"].contains(&token))
    {
        return "test".to_string();
    }
    if has_phrase(&["design only", "do not implement"]) {
        return "plan".to_string();
    }
    if text
        .split_whitespace()
        .any(|token| ["plan", "brainstorm", "proposal"].contains(&token))
    {
        return "plan".to_string();
    }
    if has_phrase(&["check whether", "confirm whether", "validate whether"]) {
        return "verify".to_string();
    }
    if text
        .split_whitespace()
        .any(|token| ["verify", "validate", "confirm", "check"].contains(&token))
    {
        return "verify".to_string();
    }
    if text.split_whitespace().any(|token| {
        [
            "implement",
            "create",
            "modify",
            "change",
            "patch",
            "update",
            "fix",
            "write",
        ]
        .contains(&token)
    }) {
        return "patch".to_string();
    }
    "patch".to_string()
}

fn build_coordinator_spec(
    template: &CoordinatorTemplate,
    template_name: &str,
    request: &str,
    cwd: &str,
    exact_artifacts: Vec<ArtifactExpectation>,
    contains_artifacts: Vec<ArtifactExpectation>,
) -> Result<Value, String> {
    let final_artifacts = exact_artifacts
        .into_iter()
        .chain(contains_artifacts)
        .map(artifact_to_value)
        .collect::<Vec<_>>();
    let last_index = template.steps.len().saturating_sub(1);
    let steps = template
        .steps
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            serde_json::json!({
                "name": raw.name,
                "worker": raw.worker,
                "cwd": cwd,
                "prompt": raw.prompt.replace("{request}", request),
                "artifacts": if index == last_index { final_artifacts.clone() } else { vec![] },
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "task_id": format!("{}-{}", utc_stamp(), template_name),
        "template": template_name,
        "request": request,
        "steps": steps,
    }))
}

fn artifact_to_value(artifact: ArtifactExpectation) -> Value {
    let mut object = Map::new();
    object.insert("path".to_string(), Value::from(artifact.path));
    if let Some(exact_text) = artifact.exact_text {
        object.insert("exact_text".to_string(), Value::from(exact_text));
    }
    if let Some(must_contain) = artifact.must_contain {
        object.insert("must_contain".to_string(), Value::from(must_contain));
    }
    Value::Object(object)
}

fn push_unique(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|existing| existing == &value) {
        items.push(value);
    }
}

fn read_required_string(object: &Map<String, Value>, key: &str) -> Result<String, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
        .ok_or(format!("spec is missing {key}"))
}

fn read_optional_u32(object: &Map<String, Value>, key: &str) -> Option<u32> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn read_stdin_text() -> Result<String, String> {
    let mut content = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut content)
        .map_err(|error| error.to_string())?;
    Ok(content)
}

fn now_system_time() -> SystemTime {
    SystemTime::now()
}

fn iso_z(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let output = Command::new("date")
        .args(["-u", "-d", &format!("@{seconds}"), "+%Y-%m-%dT%H:%M:%SZ"])
        .output();
    match output {
        Ok(result) if result.status.success() => {
            String::from_utf8_lossy(&result.stdout).trim().to_string()
        }
        _ => format!("{seconds}Z"),
    }
}

fn utc_stamp() -> String {
    let output = Command::new("date")
        .args(["-u", "+%Y%m%dT%H%M%SZ"])
        .output();
    match output {
        Ok(result) if result.status.success() => {
            String::from_utf8_lossy(&result.stdout).trim().to_string()
        }
        _ => generated_task_id(),
    }
}

fn generated_task_id() -> String {
    format!(
        "{}-rack-task",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}

fn duration_seconds(started: SystemTime, finished: SystemTime) -> f64 {
    finished
        .duration_since(started)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use rack_ai_application::WorkerBinding;
    use rack_ai_application::WorkerCatalog;

    use super::choose_template;
    use super::extract_worker_ids;
    use super::normalize_submit_spec;

    #[test]
    fn normalizes_step_spec_without_explicit_placement() {
        let normalized = normalize_submit_spec(
            r#"{
  "task_id": "task-1",
  "steps": [{"worker": "local-coder", "cwd": "/tmp", "prompt": "Do it"}]
}"#,
            &FakeWorkerCatalog,
            None,
            None,
        )
        .unwrap();

        assert_eq!(normalized.task_id, "task-1");
        assert_eq!(normalized.max_attempts, 1);
        assert_eq!(normalized.timeout_seconds, 900);
        assert_eq!(
            normalized.placement.worker_ids(),
            ["local-coder".to_string()]
        );
        assert!(normalized.spec_json.contains("placement"));
    }

    #[test]
    fn normalizes_dag_spec_and_builds_initial_dag_state() {
        let normalized = normalize_submit_spec(
            r#"{
  "task_id": "task-dag",
  "dag": {
    "nodes": [
      {"id": "plan", "worker": "local-primary", "cwd": "/tmp", "prompt": "Plan"},
      {"id": "code", "worker": "local-coder", "cwd": "/tmp", "prompt": "Code", "depends_on": ["plan"]}
    ]
  }
}"#,
            &FakeWorkerCatalog,
            Some(2),
            Some(120),
        )
        .unwrap();

        assert_eq!(normalized.max_attempts, 2);
        assert_eq!(normalized.timeout_seconds, 120);
        assert!(normalized.dag_run_state.is_some());
        assert!(normalized.spec_json.contains("local-primary"));
    }

    #[test]
    fn rejects_dag_dependency_on_unknown_node() {
        let value = serde_json::json!({
            "dag": {
                "nodes": [
                    {"id": "verify", "worker": "local-primary", "cwd": "/tmp", "prompt": "Verify", "depends_on": ["missing"]}
                ]
            }
        });
        let error = extract_worker_ids(value.as_object().unwrap()).unwrap_err();
        assert_eq!(
            error,
            "dag node verify depends on unknown node missing".to_string()
        );
    }

    #[test]
    fn chooses_test_template_for_smoke_requests() {
        assert_eq!(choose_template("please run smoke test on this"), "test");
    }

    struct FakeWorkerCatalog;

    impl WorkerCatalog for FakeWorkerCatalog {
        fn resolve(&self, worker_id: &str) -> Result<WorkerBinding, String> {
            match worker_id {
                "local-primary" => Ok(WorkerBinding::new(
                    worker_id.to_string(),
                    "gpu-4060ti".to_string(),
                    "gemma4-12b-local-primary".to_string(),
                    "jcode".to_string(),
                )),
                "local-coder" => Ok(WorkerBinding::new(
                    worker_id.to_string(),
                    "gpu-2060".to_string(),
                    "qwen25-coder-3b-awq-local-coder".to_string(),
                    "vllm".to_string(),
                )),
                _ => Err(format!("unknown worker: {worker_id}")),
            }
        }
    }
}
