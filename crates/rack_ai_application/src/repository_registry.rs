use std::path::Path;

use rack_ai_domain::RepositoryId;

use crate::ExecutorConfig;
use crate::RegisteredRepository;
use crate::WorkspaceRoot;

pub trait RepositoryRegistry {
    fn workspace_root(&self) -> Result<WorkspaceRoot, String>;
    fn executor_config(&self) -> Result<ExecutorConfig, String>;
    fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String>;

    fn resolve_target(
        &self,
        id: &RepositoryId,
        requested_root: Option<&Path>,
    ) -> Result<RegisteredRepository, String> {
        if requested_root.is_some() {
            return Err(format!("repository {} is not registered", id.value()));
        }
        self.find(id)
    }
}
