use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::GitSha;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateChangeWorktreeRequest {
    repository_root: PathBuf,
    base_sha: GitSha,
    branch_name: String,
    worktree_path: PathBuf,
}

impl CreateChangeWorktreeRequest {
    pub fn new(repository_root: PathBuf, base_sha: GitSha) -> Self {
        Self {
            repository_root,
            base_sha,
            branch_name: String::new(),
            worktree_path: PathBuf::new(),
        }
    }

    pub fn with_branch_name(mut self, branch_name: String) -> Self {
        self.branch_name = branch_name;
        self
    }

    pub fn with_worktree_path(mut self, worktree_path: PathBuf) -> Self {
        self.worktree_path = worktree_path;
        self
    }

    pub fn repository_root(&self) -> &Path {
        self.repository_root.as_path()
    }

    pub fn base_sha(&self) -> &GitSha {
        &self.base_sha
    }

    pub fn branch_name(&self) -> &str {
        self.branch_name.as_str()
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }
}

#[cfg(test)]
mod tests {
    use super::CreateChangeWorktreeRequest;
    use rack_ai_domain::GitSha;
    use std::path::PathBuf;

    #[test]
    fn stores_worktree_identity() {
        let request = CreateChangeWorktreeRequest::new(
            PathBuf::from("/srv/projects/adaptos"),
            GitSha::new("b".repeat(40)).unwrap(),
        )
        .with_branch_name("rack/change-1".to_string())
        .with_worktree_path(PathBuf::from("/srv/rack-workspaces/1/repo"));
        assert_eq!(request.branch_name(), "rack/change-1");
    }
}
