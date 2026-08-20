use std::path::PathBuf;

use rack_ai_application::ApprovedCommandPolicy;
use rack_ai_application::ExecutorConfig;
use rack_ai_application::RegisteredRepository;
use rack_ai_application::RepositoryRegistry;
use rack_ai_application::WorkspaceRoot;
use rack_ai_domain::GitRef;
use rack_ai_domain::RepositoryId;

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
        WorkspaceRoot::new(PathBuf::from(self.load_document()?.workspace_root))
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
        Ok(
            RegisteredRepository::new(id.clone(), PathBuf::from(record.root))?
                .with_default_base_ref(GitRef::new(record.default_base_ref)?)
                .with_enabled(record.enabled),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::FileSystemRepositoryRegistry;
    use crate::RegistryPaths;
    use rack_ai_application::RepositoryRegistry;
    use rack_ai_domain::RepositoryId;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn finds_registered_repository() {
        let root = temp_root();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/repositories.json"),
            r#"{
                "workspace_root": "/tmp/workspaces",
                "executor": {"image": "rust:bookworm"},
                "repositories": [{"id": "adaptos", "root": "/tmp/adaptos"}]
            }"#,
        )
        .unwrap();
        let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(root));
        let found = registry
            .find(&RepositoryId::new("adaptos".to_string()).unwrap())
            .unwrap();
        assert_eq!(found.root(), PathBuf::from("/tmp/adaptos"));
        assert!(
            registry
                .find(&RepositoryId::new("missing".to_string()).unwrap())
                .is_err()
        );
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
