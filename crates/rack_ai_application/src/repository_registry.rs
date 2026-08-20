use rack_ai_domain::RepositoryId;

use crate::ExecutorConfig;
use crate::RegisteredRepository;
use crate::WorkspaceRoot;

pub trait RepositoryRegistry {
    fn workspace_root(&self) -> Result<WorkspaceRoot, String>;
    fn executor_config(&self) -> Result<ExecutorConfig, String>;
    fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String>;
}
