use std::path::Path;
use std::path::PathBuf;

use crate::WorkspacePath;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteFileRequest {
    worktree_path: PathBuf,
    path: WorkspacePath,
    content: String,
    timeout_seconds: u32,
}

impl WriteFileRequest {
    pub fn new(worktree_path: PathBuf, path: WorkspacePath) -> Self {
        Self {
            worktree_path,
            path,
            content: String::new(),
            timeout_seconds: 30,
        }
    }

    pub fn with_content(mut self, content: String) -> Self {
        self.content = content;
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

    pub fn content(&self) -> &str {
        self.content.as_str()
    }

    pub fn timeout_seconds(&self) -> u32 {
        self.timeout_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::WriteFileRequest;
    use crate::WorkspacePath;
    use std::path::PathBuf;

    #[test]
    fn stores_workspace_relative_write() {
        let request = WriteFileRequest::new(
            PathBuf::from("/tmp/repo"),
            WorkspacePath::parse("src/lib.rs").unwrap(),
        )
        .with_content("fn main() {}".to_string());
        assert_eq!(request.path().relative(), "src/lib.rs");
    }
}
