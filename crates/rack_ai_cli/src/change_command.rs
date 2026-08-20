use std::fs;
use std::path::PathBuf;

use rack_ai_application::CoderRunRequest;
use rack_ai_application::CoderWorkspaceContext;
use rack_ai_application::ExecuteChange;
use rack_ai_application::ExecuteChangeDependencies;
use rack_ai_application::ExecuteChangeRequest;
use rack_ai_application::RepositoryRegistry;
use rack_ai_domain::AllowedPath;
use rack_ai_domain::AllowedPaths;
use rack_ai_infrastructure::DirectCoderWorker;
use rack_ai_infrastructure::FileSystemChangeManifestRepository;
use rack_ai_infrastructure::FileSystemRepositoryRegistry;
use rack_ai_infrastructure::GitCommandWorktree;
use rack_ai_infrastructure::PodmanWorkspaceExecutor;
use rack_ai_infrastructure::RegistryPaths;
use rack_ai_infrastructure::RepositoryPaths;
use rack_ai_infrastructure::WorkspaceCoderToolRunner;

pub fn run(repo_root: PathBuf, state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let mut spec_path: Option<PathBuf> = None;
    let mut run_checks = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
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
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root));
    let git = GitCommandWorktree;
    let manifests = FileSystemChangeManifestRepository::new(RepositoryPaths::new(state_root));
    let policy = registry.command_policy()?;
    let executor = if run_checks {
        Some(PodmanWorkspaceExecutor::new(registry.executor_config()?))
    } else {
        None
    };
    let service = ExecuteChange::new(ExecuteChangeDependencies {
        registry: &registry,
        command_policy: &policy,
        git: &git,
        manifests: &manifests,
        executor: executor
            .as_ref()
            .map(|item| item as &dyn rack_ai_application::WorkspaceExecutor),
    });
    let result = service.execute(ExecuteChangeRequest {
        document,
        run_checks,
    })?;
    println!("change_id: {}", result.packet.change_id());
    println!("branch: {}", result.packet.branch());
    println!("worktree: {}", result.packet.worktree_path());
    println!("base_sha: {}", result.packet.base_sha());
    let status = serde_json::to_value(result.packet.status()).map_err(|error| error.to_string())?;
    println!("status: {}", status.as_str().unwrap_or("unknown"));
    println!("packet: {}", result.packet_path);
    if let Some(error) = result.packet.last_error() {
        eprintln!("{error}");
    }
    Ok(if result.succeeded() { 0 } else { 1 })
}

pub fn run_coder_in_podman(
    repo_root: PathBuf,
    workdir: PathBuf,
    prompt_text: &str,
    max_turns: usize,
    allowed_paths: Vec<String>,
) -> Result<String, String> {
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root));
    let executor = PodmanWorkspaceExecutor::new(registry.executor_config()?);
    let allowed = AllowedPaths::new(
        allowed_paths
            .into_iter()
            .map(AllowedPath::new)
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let runner =
        WorkspaceCoderToolRunner::new(&executor, CoderWorkspaceContext::new(workdir, allowed));
    DirectCoderWorker::local_default().execute_with_runner(
        &CoderRunRequest::new(prompt_text.to_string(), max_turns)?,
        &runner,
    )
}
