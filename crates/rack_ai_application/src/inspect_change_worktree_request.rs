use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::GitSha;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectChangeWorktreeRequest {
    worktree_path: PathBuf,
    expected_base_sha: GitSha,
}

impl InspectChangeWorktreeRequest {
    pub fn new(worktree_path: PathBuf, expected_base_sha: GitSha) -> Self {
        Self {
            worktree_path,
            expected_base_sha,
        }
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn expected_base_sha(&self) -> &GitSha {
        &self.expected_base_sha
    }
}

#[cfg(test)]
mod tests {
    use super::InspectChangeWorktreeRequest;
    use rack_ai_domain::GitSha;
    use std::path::PathBuf;

    #[test]
    fn stores_expected_baseline() {
        let request = InspectChangeWorktreeRequest::new(
            PathBuf::from("/tmp/repo"),
            GitSha::new("c".repeat(40)).unwrap(),
        );
        assert_eq!(request.expected_base_sha().value(), "c".repeat(40));
    }
}
