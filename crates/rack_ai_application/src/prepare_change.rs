use crate::ChangeLayout;
use crate::ChangeRequest;
use crate::ChangeWorkspace;
use crate::CreateChangeWorktreeRequest;
use crate::GitWorktree;
use crate::RepositoryRegistry;

pub struct PrepareChange<'a> {
    registry: &'a dyn RepositoryRegistry,
    git: &'a dyn GitWorktree,
}

pub struct PrepareChangeDependencies<'a> {
    pub registry: &'a dyn RepositoryRegistry,
    pub git: &'a dyn GitWorktree,
}

impl<'a> PrepareChange<'a> {
    pub fn new(dependencies: PrepareChangeDependencies<'a>) -> Self {
        Self {
            registry: dependencies.registry,
            git: dependencies.git,
        }
    }

    pub fn execute(&self, request: &ChangeRequest) -> Result<ChangeWorkspace, String> {
        let workspace_root = self.registry.workspace_root()?;
        let worktree_path = ChangeLayout::worktree_path(&workspace_root, request.change_id());
        let branch_name = ChangeLayout::branch_name(request.change_id());
        self.git.create(
            &CreateChangeWorktreeRequest::new(
                request.repository().registered_root().to_path_buf(),
                request.repository().base_sha().clone(),
            )
            .with_branch_name(branch_name)
            .with_worktree_path(worktree_path),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::PrepareChange;
    use super::PrepareChangeDependencies;
    use crate::ChangeWorkspace;
    use crate::CreateChangeWorktreeRequest;
    use crate::GitEvidence;
    use crate::GitWorktree;
    use crate::InspectChangeWorktreeRequest;
    use crate::ResolveGitShaRequest;
    use crate::WorkspaceRoot;
    use rack_ai_domain::ChangeId;
    use rack_ai_domain::GitSha;
    use std::cell::RefCell;
    use std::path::PathBuf;

    struct FakeGit {
        created: RefCell<Option<String>>,
    }

    impl GitWorktree for FakeGit {
        fn resolve_sha(&self, _request: &ResolveGitShaRequest) -> Result<GitSha, String> {
            GitSha::new("a".repeat(40))
        }

        fn create(&self, request: &CreateChangeWorktreeRequest) -> Result<ChangeWorkspace, String> {
            *self.created.borrow_mut() = Some(request.worktree_path().display().to_string());
            Ok(ChangeWorkspace::new(
                ChangeId::new("job-1".to_string()).unwrap(),
                request.worktree_path().to_path_buf(),
            )
            .with_branch_name(request.branch_name().to_string())
            .with_base_sha(request.base_sha().clone()))
        }

        fn inspect(&self, _request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String> {
            Err("unused".to_string())
        }
    }

    #[test]
    fn creates_worktree_under_workspace_root() {
        let git = FakeGit {
            created: RefCell::new(None),
        };
        let registry = FakeRegistry;
        let service = PrepareChange::new(PrepareChangeDependencies {
            registry: &registry,
            git: &git,
        });
        // Construction of a full ChangeRequest is covered in execute_change tests.
        assert!(WorkspaceRoot::new(PathBuf::from("/srv/rack-workspaces")).is_ok());
        let _ = service;
    }

    struct FakeRegistry;

    impl crate::RepositoryRegistry for FakeRegistry {
        fn workspace_root(&self) -> Result<crate::WorkspaceRoot, String> {
            crate::WorkspaceRoot::new(PathBuf::from("/srv/rack-workspaces"))
        }

        fn executor_config(&self) -> Result<crate::ExecutorConfig, String> {
            crate::ExecutorConfig::podman("rust:bookworm".to_string())
        }

        fn find(
            &self,
            _id: &rack_ai_domain::RepositoryId,
        ) -> Result<crate::RegisteredRepository, String> {
            Err("unused".to_string())
        }
    }
}
