use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use rack_ai_application::ApprovedCommandPolicy;
use rack_ai_application::EnvironmentResourceMount;
use rack_ai_application::ExecutorConfig;
use rack_ai_application::RegisteredRepository;
use rack_ai_application::RepositoryRegistry;
use rack_ai_application::WorkspaceRoot;
use rack_ai_domain::GitRef;
use rack_ai_domain::RepositoryId;

use crate::GitCommand;
use crate::RegistryPaths;
use crate::RepositoriesDocument;
use crate::RepositoryRecord;
use crate::TrustedDynamicRootRecord;
use crate::TrustedEnvironmentRootRecord;

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
        WorkspaceRoot::new(PathBuf::from(self.load_document()?.workspace_root))
    }

    fn executor_config(&self) -> Result<ExecutorConfig, String> {
        let executor = self.load_document()?.executor;
        match executor.backend.as_str() {
            "podman" => Ok(ExecutorConfig::podman(executor.image)?
                .with_workspace_mount(executor.workspace_path)
                .with_memory(executor.memory)
                .with_pids_limit(executor.pids_limit)),
            "host" => Ok(ExecutorConfig::host()),
            value => Err(format!("unsupported executor backend: {value}")),
        }
    }

    fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
        let document = self.load_document()?;
        let record = repository_record(&document, id)
            .ok_or(format!("repository {} is not registered", id.value()))?;
        build_static_repository(self.paths.root(), id, record)
    }

    fn resolve_target(
        &self,
        id: &RepositoryId,
        requested_root: Option<&Path>,
    ) -> Result<RegisteredRepository, String> {
        let document = self.load_document()?;
        if let Some(record) = repository_record(&document, id) {
            let repository = build_static_repository(self.paths.root(), id, record)?;
            if let Some(path) = requested_root {
                assert_requested_root_matches_registered(path, repository.root())?;
            }
            return Ok(repository);
        }
        let requested_root =
            requested_root.ok_or(format!("repository {} is not registered", id.value()))?;
        authorize_dynamic_repository(self.paths.root(), &document, id, requested_root)
    }

    fn authorize_environment_resources(
        &self,
        requested_paths: &[String],
    ) -> Result<Vec<EnvironmentResourceMount>, String> {
        let document = self.load_document()?;
        requested_paths
            .iter()
            .map(|path| {
                authorize_environment_resource(self.paths.root(), &document, Path::new(path))
            })
            .collect()
    }
}

fn repository_record<'a>(
    document: &'a RepositoriesDocument,
    id: &RepositoryId,
) -> Option<&'a RepositoryRecord> {
    document
        .repositories
        .iter()
        .find(|item| item.id == id.value())
}

fn build_static_repository(
    live_context_root: &Path,
    id: &RepositoryId,
    record: &RepositoryRecord,
) -> Result<RegisteredRepository, String> {
    let registered_root = PathBuf::from(&record.root);
    assert_not_live_repository_target(live_context_root, &registered_root)?;
    Ok(RegisteredRepository::new(id.clone(), registered_root)?
        .with_default_base_ref(GitRef::new(record.default_base_ref.clone())?)
        .with_enabled(record.enabled))
}

fn authorize_dynamic_repository(
    live_context_root: &Path,
    document: &RepositoriesDocument,
    id: &RepositoryId,
    requested_root: &Path,
) -> Result<RegisteredRepository, String> {
    let canonical_requested = canonical_repository_root(requested_root, "dynamic repository root")?;
    assert_not_live_repository_target(live_context_root, &canonical_requested)?;
    if trusted_dynamic_root(document, &canonical_requested)?.is_none() {
        return Err(format!(
            "repository {} root {} is outside trusted dynamic roots",
            id.value(),
            requested_root.display()
        ));
    }
    RegisteredRepository::new(id.clone(), canonical_requested)
}

fn authorize_environment_resource(
    live_context_root: &Path,
    document: &RepositoriesDocument,
    requested_path: &Path,
) -> Result<EnvironmentResourceMount, String> {
    let canonical_requested = canonical_existing_path(requested_path, "environment resource path")?;
    assert_not_live_environment_target(live_context_root, &canonical_requested)?;
    if trusted_environment_root(document, &canonical_requested)?.is_none() {
        return Err(format!(
            "environment resource path {} is outside trusted environment roots",
            requested_path.display()
        ));
    }
    EnvironmentResourceMount::same_path(canonical_requested)
}

fn trusted_dynamic_root<'a>(
    document: &'a RepositoriesDocument,
    requested_root: &Path,
) -> Result<Option<&'a TrustedDynamicRootRecord>, String> {
    for record in document
        .trusted_dynamic_roots
        .iter()
        .filter(|item| item.enabled)
    {
        let candidate = canonical_directory_root(
            Path::new(&record.root),
            &format!("trusted dynamic root {}", record.id),
        )?;
        if requested_root != candidate && requested_root.starts_with(&candidate) {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

fn trusted_environment_root<'a>(
    document: &'a RepositoriesDocument,
    requested_path: &Path,
) -> Result<Option<&'a TrustedEnvironmentRootRecord>, String> {
    for record in document
        .trusted_environment_roots
        .iter()
        .filter(|item| item.enabled)
    {
        let candidate = canonical_directory_root(
            Path::new(&record.root),
            &format!("trusted environment root {}", record.id),
        )?;
        if requested_path == candidate || requested_path.starts_with(&candidate) {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

fn assert_requested_root_matches_registered(
    requested_root: &Path,
    registered_root: &Path,
) -> Result<(), String> {
    let requested = canonical_repository_root(requested_root, "repository root")?;
    let registered = canonical_repository_root(registered_root, "registered repository root")?;
    if requested == registered {
        return Ok(());
    }
    Err(format!(
        "requested repository root {} does not match registered repository root {}",
        requested_root.display(),
        registered_root.display()
    ))
}

fn assert_not_live_repository_target(
    live_context_root: &Path,
    target_root: &Path,
) -> Result<(), String> {
    let live_repo = canonical_git_toplevel_for_existing_path(live_context_root)
        .map_err(|error| format!("failed to resolve live rack-ai repository: {error}"))?;
    let target_repo = canonical_git_toplevel_for_existing_path(target_root).map_err(|error| {
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

fn assert_not_live_environment_target(
    live_context_root: &Path,
    target_path: &Path,
) -> Result<(), String> {
    let live_repo = canonical_git_toplevel_for_existing_path(live_context_root)
        .map_err(|error| format!("failed to resolve live rack-ai repository: {error}"))?;
    match canonical_git_toplevel_for_existing_path(target_path) {
        Ok(target_repo) if target_repo == live_repo => Err(format!(
            "refusing to expose live rack-ai path as environment resource: {}",
            target_path.display()
        )),
        _ => Ok(()),
    }
}

fn canonical_repository_root(path: &Path, label: &str) -> Result<PathBuf, String> {
    assert_absolute_clean_path(path, label)?;
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        format!(
            "{label} {} could not be canonicalized: {error}",
            path.display()
        )
    })?;
    let git_root = canonical_git_toplevel_for_existing_path(&canonical).map_err(|error| {
        format!(
            "{label} {} is not a resolvable git repository: {error}",
            path.display()
        )
    })?;
    if canonical != git_root {
        return Err(format!(
            "{label} {} must resolve to the git repository root",
            path.display()
        ));
    }
    Ok(canonical)
}

fn canonical_existing_path(path: &Path, label: &str) -> Result<PathBuf, String> {
    assert_absolute_clean_path(path, label)?;
    std::fs::canonicalize(path).map_err(|error| {
        format!(
            "{label} {} could not be canonicalized: {error}",
            path.display()
        )
    })
}

fn canonical_directory_root(path: &Path, label: &str) -> Result<PathBuf, String> {
    let canonical = canonical_existing_path(path, label)?;
    if !canonical.is_dir() {
        return Err(format!("{label} {} is not a directory", path.display()));
    }
    Ok(canonical)
}

fn assert_absolute_clean_path(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{label} must be an absolute path"));
    }
    for component in path.components() {
        match component {
            Component::CurDir | Component::ParentDir => {
                return Err(format!("{label} must not contain traversal components"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn canonical_git_toplevel_for_existing_path(path: &Path) -> Result<PathBuf, String> {
    let context = if path.is_dir() {
        path
    } else {
        path.parent()
            .ok_or(format!("path {} has no parent", path.display()))?
    };
    let top_level = GitCommand::run(context, &["rev-parse", "--show-toplevel"])?;
    std::fs::canonicalize(&top_level).map_err(|error| error.to_string())
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
        write_repositories_document(&live, &[external.clone()], &[]);
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
    fn resolves_dynamic_repository_without_static_entry() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_root = root.join("trusted-projects");
        fs::create_dir_all(&trusted_root).unwrap();
        let dynamic = init_git_repo(&trusted_root.join("project-a"), "dynamic");
        write_repositories_document(&live, &[], &[trusted_root.clone()]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        assert!(
            registry
                .find(&RepositoryId::new("project-a".to_string()).unwrap())
                .is_err()
        );
        let found = registry
            .resolve_target(
                &RepositoryId::new("project-a".to_string()).unwrap(),
                Some(dynamic.as_path()),
            )
            .unwrap();
        assert_eq!(found.root(), dynamic);
    }

    #[test]
    fn rejects_dynamic_repository_outside_trusted_root() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_root = root.join("trusted-projects");
        let dynamic = init_git_repo(&root.join("outside-project"), "outside");
        fs::create_dir_all(&trusted_root).unwrap();
        write_repositories_document(&live, &[], &[trusted_root]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let error = registry
            .resolve_target(
                &RepositoryId::new("outside-project".to_string()).unwrap(),
                Some(dynamic.as_path()),
            )
            .unwrap_err();
        assert!(error.contains("outside trusted dynamic roots"));
    }

    #[test]
    fn rejects_dynamic_repository_with_traversal_path() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_root = root.join("trusted-projects");
        fs::create_dir_all(&trusted_root).unwrap();
        write_repositories_document(&live, &[], &[trusted_root.clone()]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let traversing = trusted_root.join("nested").join("..").join("project-a");
        let error = registry
            .resolve_target(
                &RepositoryId::new("project-a".to_string()).unwrap(),
                Some(traversing.as_path()),
            )
            .unwrap_err();
        assert!(error.contains("must not contain traversal components"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_outside_trusted_root() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_root = root.join("trusted-projects");
        let outside = init_git_repo(&root.join("outside-project"), "outside");
        fs::create_dir_all(&trusted_root).unwrap();
        let alias = trusted_root.join("project-link");
        symlink(&outside, &alias).unwrap();
        write_repositories_document(&live, &[], &[trusted_root]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let error = registry
            .resolve_target(
                &RepositoryId::new("project-link".to_string()).unwrap(),
                Some(alias.as_path()),
            )
            .unwrap_err();
        assert!(error.contains("outside trusted dynamic roots"));
    }

    #[test]
    fn loads_host_executor_config() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        fs::create_dir_all(live.join("workspaces")).unwrap();
        fs::create_dir_all(live.join("config")).unwrap();
        fs::write(
            live.join("config/repositories.json"),
            format!(
                r#"{{"workspace_root":"{}","executor":{{"backend":"host"}},"trusted_dynamic_roots":[],"trusted_environment_roots":[],"repositories":[]}}"#,
                live.join("workspaces").display()
            ),
        )
        .unwrap();
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let config = registry.executor_config().unwrap();
        assert_eq!(config.backend(), "host");
        assert_eq!(config.image(), None);
    }

    #[test]
    fn rejects_non_git_dynamic_repository() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_root = root.join("trusted-projects");
        let plain = trusted_root.join("not-a-git-repo");
        fs::create_dir_all(&plain).unwrap();
        write_repositories_document(&live, &[], &[trusted_root]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let error = registry
            .resolve_target(
                &RepositoryId::new("not-a-git-repo".to_string()).unwrap(),
                Some(plain.as_path()),
            )
            .unwrap_err();
        assert!(error.contains("is not a resolvable git repository"));
    }

    #[test]
    fn rejects_exact_live_repo_root() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        write_repositories_document(&live, std::slice::from_ref(&live), &[]);
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
        write_repositories_document(&live, &[child], &[]);
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
        write_repositories_document(&live, &[alias], &[]);
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
        write_repositories_document(&live, std::slice::from_ref(&clone), &[]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let found = registry
            .find(&RepositoryId::new("repo-0".to_string()).unwrap())
            .unwrap();
        assert_eq!(found.root(), clone);
    }

    #[test]
    fn static_repository_requested_root_must_match_registered_root() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let external = init_git_repo(&root.join("adaptos"), "external");
        let sibling = init_git_repo(&root.join("other"), "other");
        write_repositories_document(&live, std::slice::from_ref(&external), &[]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let error = registry
            .resolve_target(
                &RepositoryId::new("repo-0".to_string()).unwrap(),
                Some(sibling.as_path()),
            )
            .unwrap_err();
        assert!(error.contains("does not match registered repository root"));
    }

    #[test]
    fn authorizes_environment_root_within_trusted_environment_roots() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_env = temp_environment_root("shared-runtime");
        fs::create_dir_all(&trusted_env).unwrap();
        write_repositories_document_with_environment_roots(
            &live,
            &[],
            &[],
            std::slice::from_ref(&trusted_env),
        );
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let mounts = registry
            .authorize_environment_resources(&[trusted_env.display().to_string()])
            .unwrap();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].source_path(), trusted_env.as_path());
        assert_eq!(mounts[0].container_path(), trusted_env.as_path());
        assert!(mounts[0].read_only());
    }

    #[test]
    fn rejects_environment_path_outside_trusted_environment_roots() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_env = temp_environment_root("shared-runtime");
        let outside_env = temp_environment_root("other-runtime");
        fs::create_dir_all(&trusted_env).unwrap();
        fs::create_dir_all(&outside_env).unwrap();
        write_repositories_document_with_environment_roots(&live, &[], &[], &[trusted_env]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let error = registry
            .authorize_environment_resources(&[outside_env.display().to_string()])
            .unwrap_err();
        assert!(error.contains("outside trusted environment roots"));
    }

    #[test]
    fn rejects_environment_path_with_traversal() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_env = temp_environment_root("shared-runtime");
        fs::create_dir_all(&trusted_env).unwrap();
        write_repositories_document_with_environment_roots(
            &live,
            &[],
            &[],
            std::slice::from_ref(&trusted_env),
        );
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let traversing = trusted_env.join("nested").join("..").join("toolchain");
        let error = registry
            .authorize_environment_resources(&[traversing.display().to_string()])
            .unwrap_err();
        assert!(error.contains("must not contain traversal components"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_environment_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let trusted_env = temp_environment_root("shared-runtime");
        let outside_env = temp_environment_root("other-runtime");
        fs::create_dir_all(&trusted_env).unwrap();
        fs::create_dir_all(&outside_env).unwrap();
        let alias = trusted_env.join("toolchain-link");
        symlink(&outside_env, &alias).unwrap();
        write_repositories_document_with_environment_roots(
            &live,
            &[],
            &[],
            std::slice::from_ref(&trusted_env),
        );
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let error = registry
            .authorize_environment_resources(&[alias.display().to_string()])
            .unwrap_err();
        assert!(error.contains("outside trusted environment roots"));
    }

    #[test]
    fn rejects_environment_path_inside_live_rack_ai_repository() {
        let root = temp_root();
        let live = init_git_repo(&root.join("live-rack-ai"), "live");
        let live_subdir = live.join("state").join("runtime-cache");
        fs::create_dir_all(&live_subdir).unwrap();
        write_repositories_document_with_environment_roots(&live, &[], &[], &[live.clone()]);
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live));
        let error = registry
            .authorize_environment_resources(&[live_subdir.display().to_string()])
            .unwrap_err();
        assert!(error.contains("refusing to expose live rack-ai path as environment resource"));
    }

    fn write_repositories_document(
        live_root: &Path,
        repositories: &[PathBuf],
        trusted_dynamic_roots: &[PathBuf],
    ) {
        write_repositories_document_with_environment_roots(
            live_root,
            repositories,
            trusted_dynamic_roots,
            &[],
        );
    }

    fn write_repositories_document_with_environment_roots(
        live_root: &Path,
        repositories: &[PathBuf],
        trusted_dynamic_roots: &[PathBuf],
        trusted_environment_roots: &[PathBuf],
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
        let trusted_roots_json = trusted_dynamic_roots
            .iter()
            .enumerate()
            .map(|(index, path)| {
                format!(
                    r#"{{"id":"trusted-{index}","root":"{}","enabled":true}}"#,
                    path.display()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let trusted_environment_json = trusted_environment_roots
            .iter()
            .enumerate()
            .map(|(index, path)| {
                format!(
                    r#"{{"id":"environment-{index}","root":"{}","enabled":true}}"#,
                    path.display()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        fs::write(
            live_root.join("config/repositories.json"),
            format!(
                r#"{{"workspace_root":"{}","executor":{{"backend":"podman","image":"rust:bookworm"}},"trusted_dynamic_roots":[{}],"trusted_environment_roots":[{}],"repositories":[{}]}}"#,
                live_root.join("workspaces").display(),
                trusted_roots_json,
                trusted_environment_json,
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

    fn temp_environment_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = PathBuf::from("/var/tmp").join(format!("rack-ai-{label}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rack-ai-registry-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
