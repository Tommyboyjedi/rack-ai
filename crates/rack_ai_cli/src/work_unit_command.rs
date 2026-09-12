use std::fs;
use std::path::PathBuf;

use rack_ai_application::ExecuteWorkUnit;
use rack_ai_application::ExecuteWorkUnitDependencies;
use rack_ai_application::RepositoryRegistry;
use rack_ai_infrastructure::ConfiguredWorkspaceExecutor;
use rack_ai_infrastructure::FileSystemChangeManifestRepository;
use rack_ai_infrastructure::FileSystemRepositoryRegistry;
use rack_ai_infrastructure::GitCommandWorktree;
use rack_ai_infrastructure::JCodeChangeImplementer;
use rack_ai_infrastructure::RegistryPaths;
use rack_ai_infrastructure::RegistryWorkUnitWorkerSelector;
use rack_ai_infrastructure::RepositoryPaths;

pub fn run(repo_root: PathBuf, state_root: PathBuf, arguments: &[String]) -> Result<i32, String> {
    let mut spec_path: Option<PathBuf> = None;
    let mut emit_json = false;
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
                    return Err("expected a single work-unit spec path".to_string());
                }
                spec_path = Some(PathBuf::from(value));
                index += 1;
            }
        }
    }
    let spec_path = spec_path.ok_or("expected work-unit spec path")?;
    let document =
        serde_json::from_str(&fs::read_to_string(&spec_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(repo_root.clone()));
    let git = GitCommandWorktree;
    let manifests = FileSystemChangeManifestRepository::new(RepositoryPaths::new(state_root));
    let policy = registry.command_policy()?;
    let executor = ConfiguredWorkspaceExecutor::new(registry.executor_config()?)?;
    let implementer = JCodeChangeImplementer::new(RegistryPaths::new(repo_root.clone()), None);
    let selector = RegistryWorkUnitWorkerSelector::new(RegistryPaths::new(repo_root));
    let result = ExecuteWorkUnit::new(ExecuteWorkUnitDependencies {
        registry: &registry,
        command_policy: &policy,
        git: &git,
        manifests: &manifests,
        executor: Some(&executor),
        implementer: Some(&implementer),
        selector: &selector,
    })
    .execute(document)?;
    if emit_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?
        );
    } else {
        println!("workload_id: {}", result.workload_id);
        println!("work_unit_id: {}", result.work_unit_id);
        println!("change_id: {}", result.change_id);
        println!("selected_worker_id: {}", result.selected_worker_id);
        if let Some(ref provenance) = result.worker_provenance {
            println!(
                "worker_provenance: {}",
                serde_json::to_string(provenance).map_err(|error| error.to_string())?
            );
        } else {
            println!("worker_provenance: unavailable");
        }
        println!("status: {:?}", result.status);
        if let Some(ref verdict) = result.acceptance_verdict {
            println!("acceptance_verdict: {:?}", verdict);
        }
        if let Some(ref revision) = result.accepted_revision {
            println!("accepted_revision: {}", revision);
        }
        println!("packet: {}", result.packet_path);
    }
    Ok(
        if result.acceptance_verdict == Some(rack_ai_domain::AcceptanceVerdict::Approved) {
            0
        } else {
            1
        },
    )
}
