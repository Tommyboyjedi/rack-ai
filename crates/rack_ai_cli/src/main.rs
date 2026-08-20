use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;

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
use rack_ai_infrastructure::PythonRackTaskExecutor;
use rack_ai_infrastructure::RegistryPaths;
use rack_ai_infrastructure::RepositoryPaths;
use rack_ai_infrastructure::UtcDateCommandClock;
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

fn main() {
    if let Err(error) = execute() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn execute() -> Result<(), String> {
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
        run_next(paths, roots.repo_root)
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
) -> Result<(), String> {
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
    Ok(())
}

fn status(paths: RepositoryPaths, emit_json: bool) -> Result<(), String> {
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
        return Ok(());
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
    Ok(())
}

fn run_next(paths: RepositoryPaths, repo_root: PathBuf) -> Result<(), String> {
    let state_root = paths.root().to_path_buf();
    let clock = UtcDateCommandClock;
    let execution_queue_repository = FileSystemExecutionQueueRepository::new(paths.clone());
    let lease_repository = FileSystemLeaseRepository::new(paths.clone());
    let run_state_repository = FileSystemRunStateRepository::new(paths.clone());
    let task_spec_repository = FileSystemTaskSpecRepository::new(paths);
    let worker_catalog = FileSystemWorkerCatalog::new(RegistryPaths::new(repo_root.clone()));
    let task_executor = PythonRackTaskExecutor::new(repo_root, state_root);
    let service = RunNextTask::new(RunNextTaskDependencies {
        clock: &clock,
        execution_queue_repository: &execution_queue_repository,
        lease_repository: &lease_repository,
        run_state_repository: &run_state_repository,
        task_executor: &task_executor,
        task_spec_repository: &task_spec_repository,
        worker_catalog: &worker_catalog,
    });
    match service.execute()? {
        RunNextOutcome::NoQueuedTasks => println!("No queued tasks."),
        RunNextOutcome::NoAdmissibleTasks => println!("No admissible queued tasks."),
        RunNextOutcome::Succeeded(task_id) => println!("{task_id}"),
        RunNextOutcome::Requeued(task_id) => println!("Requeued {task_id}"),
        RunNextOutcome::Failed(task_id) => println!("Failed {task_id}"),
    }
    Ok(())
}

fn healthcheck(repo_root: PathBuf) -> Result<(), String> {
    let registry_repository = FileSystemRegistryRepository::new(RegistryPaths::new(repo_root));
    let probe = EndpointProbe;
    let service = HealthcheckService::new(HealthcheckServiceDependencies {
        endpoint_probe: &probe,
        registry_repository: &registry_repository,
    });
    let snapshot = service.execute()?;
    let json = serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
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

#[cfg(test)]
mod tests {
    use rack_ai_application::WorkerBinding;
    use rack_ai_application::WorkerCatalog;

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
