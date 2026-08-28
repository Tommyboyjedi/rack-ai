use std::path::Path;
use std::path::PathBuf;

use rack_ai_application::ApprovedCommandPolicy;
use rack_ai_application::ExecutorConfig;
use rack_ai_application::RegisteredRepository;
use rack_ai_application::RepositoryRegistry;
use rack_ai_application::WorkspaceRoot;
use rack_ai_domain::GitRef;
use rack_ai_domain::RepositoryId;

use crate::GitCommand;
use crate::RegistryPaths;
use crate::RepositoriesDocument;

pub struct FileSystemRepositoryRegistry {
    paths: RegistryPaths,
}

impl FileSystemRepositoryRegistry {
    pub fn new(paths: RegistryPaths) -> Self {
        Self { paths }
    }

    pub fn load_document(&self) -> Result<RepositoriesDocument, String> {
        let content = std::fs::read_to_string(self.paths.repositories_path())
            .map_err(|error| error.to_string())?;
        serde_json::from_str::<RepositoriesDocument>(&content).map_err(|error| error.to_string())
    }

    pub fn command_policy(&self) -> Result<ApprovedCommandPolicy, String> {
        let document = self.load_document()?;
        if document.approved_programs.is_empty() {
            return Ok(ApprovedCommandPolicy::default());
        }
        ApprovedCommandPolicy::new(document.approved_programs)
    }
}

impl RepositoryRegistry for FileSystemRepositoryRegistry {
    fn workspace_root(&self) -> Result<WorkspaceRoot, String> {
        let configured = PathBuf::from(self.load_document()?.workspace_root);
        WorkspaceRoot::new(resolve_workspace_root(self.paths.root(), &configured)?)
    }

    fn executor_config(&self) -> Result<ExecutorConfig, String> {
        let executor = self.load_document()?.executor;
        if executor.backend != "podman" {
            return Err(format!(
                "unsupported executor backend: {}",
                executor.backend
            ));
        }
        Ok(ExecutorConfig::podman(executor.image)?
            .with_workspace_mount(executor.workspace_path)
            .with_memory(executor.memory)
            .with_pids_limit(executor.pids_limit))
    }

    fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
        let document = self.load_document()?;
        let record = document
            .repositories
            .into_iter()
            .find(|item| item.id == id.value())
            .ok_or(format!("repository {} is not registered", id.value()))?;
        let registered_root = PathBuf::from(record.root);
        assert_not_live_repository_target(self.paths.root(), &registered_root)?;
        Ok(RegisteredRepository::new(id.clone(), registered_root)?
            .with_default_base_ref(GitRef::new(record.default_base_ref)?)
            .with_enabled(record.enabled))
    }
}

fn assert_not_live_repository_target(
    live_context_root: &Path,
    target_root: &Path,
) -> Result<(), String> {
    let live_repo = canonical_git_toplevel(live_context_root)
        .map_err(|error| format!("failed to resolve live rack-ai repository: {error}"))?;
    let target_repo = canonical_git_toplevel(target_root).map_err(|error| {
        format!(
            "registered repository root {} is not a resolvable git repository: {error}",
            target_root.display()
        )
    })?;
    if live_repo == target_repo {
        return Err(format!(
            "refusing to target the live rack-ai repository: {}",
            target_root.display()
        ));
    }
    Ok(())
}

fn canonical_git_toplevel(path: &Path) -> Result<PathBuf, String> {
    let top_level = GitCommand::run(path, &["rev-parse", "--show-toplevel"])?;
    std::fs::canonicalize(&top_level).map_err(|error| error.to_string())
}

fn resolve_workspace_root(
    live_context_root: &Path,
    configured_root: &Path,
) -> Result<PathBuf, String> {
    if !configured_root.is_absolute() {
        return Err("workspace root must be an absolute path".to_string());
    }
    let live_repo = canonical_git_toplevel(live_context_root)
        .map_err(|error| format!("failed to resolve live rack-ai repository: {error}"))?;
    let configured = resolve_path_for_containment(configured_root)?;
    if configured.starts_with(&live_repo) {
        return derive_external_workspace_root(&live_repo);
    }
    Ok(configured_root.to_path_buf())
}

fn resolve_path_for_containment(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return std::fs::canonicalize(path).map_err(|error| error.to_string());
    }
    let mut suffix = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        let name = cursor
            .file_name()
            .ok_or("workspace root has no existing ancestor".to_string())?;
        suffix.push(name.to_os_string());
        cursor = cursor
            .parent()
            .ok_or("workspace root has no existing ancestor".to_string())?;
    }
    let mut resolved = std::fs::canonicalize(cursor).map_err(|error| error.to_string())?;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn derive_external_workspace_root(live_repo: &Path) -> Result<PathBuf, String> {
    let name = live_repo
        .file_name()
        .ok_or("live rack-ai repository has no terminal directory name".to_string())?;
    let sibling = live_repo.with_file_name(format!("{}-workspaces", name.to_string_lossy()));
    Ok(sibling)
}

#[cfg(test)]
mod tests {
    use super::FileSystemRepositoryRegistry;
    use crate::RegistryPaths;
    use rack_ai_application::RepositoryRegistry;
    use rack_ai_domain::RepositoryId;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn finds_registered_repository() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let external = init_git_repo(&root.join("adaptos"), "external");
        write_repositories_document(&live, &[external.clone()]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live.clone()));
        let found = registry
            .find(&RepositoryId::new("repo-0".to_string()).unwrap())
            .unwrap();
        assert_eq!(found.root(), external);
        assert!(
            registry
                .find(&RepositoryId::new("missing".to_string()).unwrap())
                .is_err()
        );
    }

    #[test]
    fn rejects_exact_live_repo_root() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        write_repositories_document(&live, std::slice::from_ref(&live));
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live.clone()));
        let error = registry
            .find(&RepositoryId::new("repo-0".to_string()).unwrap())
            .unwrap_err();
        assert!(error.contains("refusing to target the live rack-ai repository"));
    }

    #[test]
    fn rejects_child_path_resolving_to_live_repo() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let child = live.join("state").join("foo");
        fs::create_dir_all(&child).unwrap();
        write_repositories_document(&live, &[child]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live.clone()));
        let error = registry
            .find(&RepositoryId::new("repo-0".to_string()).unwrap())
            .unwrap_err();
        assert!(error.contains("refusing to target the live rack-ai repository"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_alias_of_live_repo() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let alias = root.join("live-link");
        symlink(&live, &alias).unwrap();
        write_repositories_document(&live, &[alias]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live.clone()));
        let error = registry
            .find(&RepositoryId::new("repo-0".to_string()).unwrap())
            .unwrap_err();
        assert!(error.contains("refusing to target the live rack-ai repository"));
    }

    #[test]
    fn accepts_separate_clone_of_rack_ai() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let clone = root.join("rack-ai-clone");
        git_clone(&live, &clone);
        write_repositories_document(&live, std::slice::from_ref(&clone));
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let found = registry
            .find(&RepositoryId::new("repo-0".to_string()).unwrap())
            .unwrap();
        assert_eq!(found.root(), clone);
    }

    #[test]
    fn rewrites_nested_workspace_root_outside_live_repo() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let external = init_git_repo(&root.join("adaptos"), "external");
        write_repositories_document_with_workspace(
            &live,
            live.join("state/workspaces"),
            &[external],
        );
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live.clone()));
        let workspace = registry.workspace_root().unwrap();
        assert_eq!(
            workspace.as_path(),
            live.with_file_name("live-rack-ai-workspaces")
        );
    }

    #[test]
    fn keeps_explicit_external_workspace_root() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let external = init_git_repo(&root.join("adaptos"), "external");
        let workspaces = root.join("custom-workspaces");
        write_repositories_document_with_workspace(&live, workspaces.clone(), &[external]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let workspace = registry.workspace_root().unwrap();
        assert_eq!(workspace.as_path(), workspaces);
    }

    fn write_repositories_document(live_root: &Path, repositories: &[PathBuf]) {
        write_repositories_document_with_workspace(
            live_root,
            live_root.join("workspaces"),
            repositories,
        );
    }

    fn write_repositories_document_with_workspace(
        live_root: &Path,
        workspace_root: PathBuf,
        repositories: &[PathBuf],
    ) {
        fs::create_dir_all(live_root.join("config")).unwrap();
        let repositories_json = repositories
            .iter()
            .enumerate()
            .map(|(index, path)| {
                format!(
                    r#"{{"id":"repo-{index}","root":"{}","default_base_ref":"main"}}"#,
                    path.display()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        fs::write(
            live_root.join("config/repositories.json"),
            format!(
                r#"{{"workspace_root":"{}","executor":{{"backend":"podman","image":"rust:bookworm"}},"repositories":[{}]}}"#,
                workspace_root.display(),
                repositories_json
            ),
        )
        .unwrap();
    }

    fn git_clone(source: &Path, target: &Path) {
        let status = Command::new("git")
            .args(["clone", source.to_str().unwrap(), target.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn init_git_repo(path: &Path, marker: &str) -> PathBuf {
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(
            path.join("src/lib.rs"),
            format!("pub fn marker() -> &'static str {{ \"{marker}\" }}\n"),
        )
        .unwrap();
        let status = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
        path.to_path_buf()
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-repos-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
