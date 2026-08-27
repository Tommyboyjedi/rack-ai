use std::fs;
use std::path::PathBuf;

use rack_ai_application::ChangeExecutionMode;
use rack_ai_application::ExecuteChange;
use rack_ai_application::ExecuteChangeDependencies;
use rack_ai_application::ExecuteChangeRequest;
use rack_ai_application::RepositoryRegistry;
use rack_ai_infrastructure::FileSystemChangeManifestRepository;
use rack_ai_infrastructure::FileSystemRepositoryRegistry;
use rack_ai_infrastructure::GitCommandWorktree;
use rack_ai_infrastructure::JCodeChangeImplementer;
use rack_ai_infrastructure::JCodeWorkerConfigResolver;
use rack_ai_infrastructure::PodmanWorkspaceExecutor;
use rack_ai_infrastructure::RegistryPaths;
use rack_ai_infrastructure::RepositoryPaths;

pub fn run(repo_root: PathBuf, state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let mut spec_path: Option<PathBuf> = None;
    let mut prepare_only = false;
    let mut run_checks = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--prepare-only" => {
                prepare_only = true;
                index += 1;
            }
            "--run-checks" => {
                run_checks = true;
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
    let spec_path = spec_path.ok_or("expected change request spec path")?;
    let document =
        serde_json::from_str(&fs::read_to_string(&spec_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let mode = if prepare_only {
        ChangeExecutionMode::PrepareOnly
    } else if run_checks {
        ChangeExecutionMode::ChecksOnly
    } else {
        ChangeExecutionMode::ImplementAndVerify
    };
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root.clone()));
    let runtime_resolver = JCodeWorkerConfigResolver::new(RegistryPaths::new(repo_root.clone()));
    let git = GitCommandWorktree;
    let manifests = FileSystemChangeManifestRepository::new(RepositoryPaths::new(state_root));
    let policy = registry.command_policy()?;
    let executor_config = if mode.runs_checks() || mode.runs_implementer() {
        Some(registry.executor_config()?)
    } else {
        None
    };
    let executor = executor_config.clone().map(PodmanWorkspaceExecutor::new);
    let default_worker = if mode.runs_implementer() {
        Some(runtime_resolver.resolve_default_implementer()?)
    } else {
        None
    };
    let implementer = default_worker.map(|worker| {
        JCodeChangeImplementer::new(RegistryPaths::new(repo_root.clone()), Some(worker))
    });
    let service = ExecuteChange::new(ExecuteChangeDependencies {
        registry: &registry,
        command_policy: &policy,
        git: &git,
        manifests: &manifests,
        executor: executor
            .as_ref()
            .map(|item| item as &dyn rack_ai_application::WorkspaceExecutor),
        implementer: implementer
            .as_ref()
            .map(|item| item as &dyn rack_ai_application::ChangeImplementer),
    });
    let result = service.execute(ExecuteChangeRequest {
        document,
        mode,
        selected_worker: None,
    })?;
    println!("change_id: {}", result.packet.change_id());
    println!("branch: {}", result.packet.branch());
    println!("worktree: {}", result.packet.worktree_path());
    println!("base_sha: {}", result.packet.base_sha());
    let status = serde_json::to_value(result.packet.status()).map_err(|error| error.to_string())?;
    println!("status: {}", status.as_str().unwrap_or("unknown"));
    if let Some(verdict) = result.packet.acceptance_verdict() {
        let verdict = serde_json::to_value(verdict).map_err(|error| error.to_string())?;
        println!(
            "acceptance_verdict: {}",
            verdict.as_str().unwrap_or("unknown")
        );
    }
    println!("packet: {}", result.packet_path);
    if let Some(error) = result.packet.last_error() {
        eprintln!("{error}");
    }
    Ok(if result.succeeded() { 0 } else { 1 })
}
