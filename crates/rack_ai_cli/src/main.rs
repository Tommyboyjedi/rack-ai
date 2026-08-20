use std::env;
use std::fs;
use std::path::PathBuf;

use rack_ai_application::InspectStatus;
use rack_ai_application::InspectStatusDependencies;
use rack_ai_application::RunNextOutcome;
use rack_ai_application::RunNextTask;
use rack_ai_application::RunNextTaskDependencies;
use rack_ai_application::SubmitTask;
use rack_ai_application::SubmitTaskDependencies;
use rack_ai_application::SubmitTaskRequest;
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
use serde::Deserialize;

struct CommandRoots {
    repo_root: PathBuf,
    state_root: PathBuf,
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
        submit(paths, PathBuf::from(spec_path))
    } else if command == "status" {
        status(paths)
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

fn submit(paths: RepositoryPaths, spec_path: PathBuf) -> Result<(), String> {
    let spec_json = fs::read_to_string(spec_path).map_err(|error| error.to_string())?;
    let spec = serde_json::from_str::<SubmitSpec>(&spec_json).map_err(|error| error.to_string())?;
    let run_state_repository = FileSystemRunStateRepository::new(paths.clone());
    let task_spec_repository = FileSystemTaskSpecRepository::new(paths);
    let service = SubmitTask::new(SubmitTaskDependencies {
        run_state_repository: &run_state_repository,
        task_spec_repository: &task_spec_repository,
    });
    let request = SubmitTaskRequest {
        spec_json,
        run_state: RunStateDraft {
            task_id: TaskId::new(spec.task_id)?,
            attempt_limit: AttemptLimit::new(spec.max_attempts)?,
            timeout_seconds: TimeoutSeconds::new(spec.timeout_seconds)?,
            placement: spec.placement.into_domain(),
        },
    };
    let run_state = service.execute(request)?;
    println!("{}", run_state.task_id().value());
    Ok(())
}

fn status(paths: RepositoryPaths) -> Result<(), String> {
    let queue_state_repository = FileSystemQueueStateRepository::new(paths.clone());
    let run_state_repository = FileSystemRunStateRepository::new(paths);
    let service = InspectStatus::new(InspectStatusDependencies {
        queue_state_repository: &queue_state_repository,
        run_state_repository: &run_state_repository,
    });
    let snapshot = service.execute()?;
    let json = serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn run_next(paths: RepositoryPaths, repo_root: PathBuf) -> Result<(), String> {
    let state_root = paths.root().to_path_buf();
    let execution_queue_repository = FileSystemExecutionQueueRepository::new(paths.clone());
    let lease_repository = FileSystemLeaseRepository::new(paths.clone());
    let run_state_repository = FileSystemRunStateRepository::new(paths.clone());
    let task_spec_repository = FileSystemTaskSpecRepository::new(paths);
    let worker_catalog = FileSystemWorkerCatalog::new(RegistryPaths::new(repo_root.clone()));
    let task_executor = PythonRackTaskExecutor::new(repo_root, state_root);
    let service = RunNextTask::new(RunNextTaskDependencies {
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

#[derive(Deserialize)]
struct SubmitSpec {
    task_id: String,
    max_attempts: u32,
    timeout_seconds: u32,
    placement: SubmitPlacement,
}

#[derive(Deserialize)]
struct SubmitPlacement {
    worker_ids: Vec<String>,
    resource_ids: Vec<String>,
    model_ids: Vec<String>,
    backends: Vec<String>,
}

impl SubmitPlacement {
    fn into_domain(self) -> Placement {
        Placement::new(self.worker_ids, self.resource_ids)
            .with_models(self.model_ids)
            .with_backends(self.backends)
    }
}
