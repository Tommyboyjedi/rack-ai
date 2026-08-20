use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::AllowedPaths;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoderWorkspaceContext {
    worktree_path: PathBuf,
    allowed_paths: AllowedPaths,
}

impl CoderWorkspaceContext {
    pub fn new(worktree_path: PathBuf, allowed_paths: AllowedPaths) -> Self {
        Self {
            worktree_path,
            allowed_paths,
        }
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn allowed_paths(&self) -> &AllowedPaths {
        &self.allowed_paths
    }
}

#[cfg(test)]
mod tests {
    use super::CoderWorkspaceContext;
    use rack_ai_domain::AllowedPath;
    use rack_ai_domain::AllowedPaths;
    use std::path::PathBuf;

    #[test]
    fn stores_worktree_and_policy() {
        let context = CoderWorkspaceContext::new(
            PathBuf::from("/tmp/repo"),
            AllowedPaths::new(vec![AllowedPath::new("src".to_string()).unwrap()]).unwrap(),
        );
        assert!(context.allowed_paths().allows("src/lib.rs"));
    }
}
