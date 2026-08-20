use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::ChangeId;
use rack_ai_domain::GitSha;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeWorkspace {
    change_id: ChangeId,
    branch_name: String,
    worktree_path: PathBuf,
    base_sha: GitSha,
}

impl ChangeWorkspace {
    pub fn new(change_id: ChangeId, worktree_path: PathBuf) -> Self {
        Self {
            change_id,
            branch_name: String::new(),
            worktree_path,
            base_sha: GitSha::new("0".repeat(40)).unwrap(),
        }
    }

    pub fn with_branch_name(mut self, branch_name: String) -> Self {
        self.branch_name = branch_name;
        self
    }

    pub fn with_base_sha(mut self, base_sha: GitSha) -> Self {
        self.base_sha = base_sha;
        self
    }

    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    pub fn branch_name(&self) -> &str {
        self.branch_name.as_str()
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn base_sha(&self) -> &GitSha {
        &self.base_sha
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeWorkspace;
    use rack_ai_domain::ChangeId;
    use rack_ai_domain::GitSha;
    use std::path::PathBuf;

    #[test]
    fn stores_worktree_metadata() {
        let workspace = ChangeWorkspace::new(
            ChangeId::new("job-1".to_string()).unwrap(),
            PathBuf::from("/srv/rack-workspaces/job-1/repo"),
        )
        .with_branch_name("rack/change-job-1".to_string())
        .with_base_sha(GitSha::new("d".repeat(40)).unwrap());
        assert_eq!(workspace.branch_name(), "rack/change-job-1");
    }
}
