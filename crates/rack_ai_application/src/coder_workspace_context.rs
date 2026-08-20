use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::AllowedPaths;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoderWorkspaceContext {
    worktree_path: PathBuf,
    allowed_paths: AllowedPaths,
    timeout_seconds: u32,
}

impl CoderWorkspaceContext {
    pub fn new(worktree_path: PathBuf, allowed_paths: AllowedPaths) -> Self {
        Self {
            worktree_path,
            allowed_paths,
            timeout_seconds: 30,
        }
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u32) -> Self {
        self.timeout_seconds = timeout_seconds.max(1);
        self
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn allowed_paths(&self) -> &AllowedPaths {
        &self.allowed_paths
    }

    pub fn timeout_seconds(&self) -> u32 {
        self.timeout_seconds
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
