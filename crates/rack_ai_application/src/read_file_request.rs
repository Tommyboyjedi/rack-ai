use std::path::Path;
use std::path::PathBuf;

use crate::WorkspacePath;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadFileRequest {
    worktree_path: PathBuf,
    path: WorkspacePath,
    start_line: u64,
    limit: u64,
    timeout_seconds: u32,
}

impl ReadFileRequest {
    pub fn new(worktree_path: PathBuf, path: WorkspacePath) -> Self {
        Self {
            worktree_path,
            path,
            start_line: 1,
            limit: 400,
            timeout_seconds: 30,
        }
    }

    pub fn with_range(mut self, start_line: u64, limit: u64) -> Self {
        self.start_line = start_line.max(1);
        self.limit = limit.max(1);
        self
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u32) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn path(&self) -> &WorkspacePath {
        &self.path
    }

    pub fn start_line(&self) -> u64 {
        self.start_line
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }

    pub fn timeout_seconds(&self) -> u32 {
        self.timeout_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::ReadFileRequest;
    use crate::WorkspacePath;
    use std::path::PathBuf;

    #[test]
    fn stores_read_range() {
        let request = ReadFileRequest::new(
            PathBuf::from("/tmp/repo"),
            WorkspacePath::parse("README.md").unwrap(),
        )
        .with_range(2, 10);
        assert_eq!(request.start_line(), 2);
        assert_eq!(request.limit(), 10);
    }
}
