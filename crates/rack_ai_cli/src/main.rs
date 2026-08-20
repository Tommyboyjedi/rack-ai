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
use rack_ai_infrastructure::FileSystemQueueStateRepository;
use rack_ai_infrastructure::FileSystemRegistryRepository;
use rack_ai_infrastructure::FileSystemRunStateRepository;
use rack_ai_infrastructure::FileSystemTaskSpecRepository;
use rack_ai_infrastructure::HealthcheckService;
use rack_ai_infrastructure::HealthcheckServiceDependencies;
use rack_ai_infrastructure::PythonRackTaskExecutor;
use rack_ai_infrastructure::RegistryPaths;
use rack_ai_infrastructure::RepositoryPaths;
use serde::Deserialize;

fn main() {
    if let Err(error) = execute() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn execute() -> Result<(), String> {
    let arguments = env::args().collect::<Vec<_>>();
    let command = arguments.get(1).ok_or("expected command")?;
    let root = current_root(&arguments)?;
    let paths = RepositoryPaths::new(root.clone());
    if command == "submit" {
        let spec_path = arguments.get(2).ok_or("expected spec path")?;
        submit(paths, PathBuf::from(spec_path))
    } else if command == "status" {
        status(paths)
    } else if command == "run-next" {
        run_next(paths, root)
    } else if command == "healthcheck" {
        healthcheck(root)
    } else {
        Err("unsupported command".to_string())
    }
}

fn current_root(arguments: &[String]) -> Result<PathBuf, String> {
    if let Some(index) = arguments.iter().position(|value| value == "--root") {
        let value = arguments.get(index + 1).ok_or("expected root path")?;
        return Ok(PathBuf::from(value));
    }
    env::current_dir().map_err(|error| error.to_string())
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

fn run_next(paths: RepositoryPaths, root: PathBuf) -> Result<(), String> {
    let execution_queue_repository = FileSystemExecutionQueueRepository::new(paths.clone());
    let run_state_repository = FileSystemRunStateRepository::new(paths.clone());
    let task_spec_repository = FileSystemTaskSpecRepository::new(paths);
    let task_executor = PythonRackTaskExecutor::new(root);
    let service = RunNextTask::new(RunNextTaskDependencies {
        execution_queue_repository: &execution_queue_repository,
        run_state_repository: &run_state_repository,
        task_executor: &task_executor,
        task_spec_repository: &task_spec_repository,
    });
    match service.execute()? {
        RunNextOutcome::NoQueuedTasks => println!("No queued tasks."),
        RunNextOutcome::Succeeded(task_id) => println!("{task_id}"),
        RunNextOutcome::Requeued(task_id) => println!("Requeued {task_id}"),
        RunNextOutcome::Failed(task_id) => println!("Failed {task_id}"),
    }
    Ok(())
}

fn healthcheck(root: PathBuf) -> Result<(), String> {
    let registry_repository = FileSystemRegistryRepository::new(RegistryPaths::new(root));
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
