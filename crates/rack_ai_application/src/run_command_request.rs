use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunCommandRequest {
    worktree_path: PathBuf,
    argv: Vec<String>,
    timeout_seconds: u32,
}

impl RunCommandRequest {
    pub fn new(worktree_path: PathBuf, argv: Vec<String>) -> Result<Self, String> {
        if argv.is_empty() {
            return Err("workspace command cannot be empty".to_string());
        }
        Ok(Self {
            worktree_path,
            argv,
            timeout_seconds: 30,
        })
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u32) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn argv(&self) -> &[String] {
        self.argv.as_slice()
    }

    pub fn timeout_seconds(&self) -> u32 {
        self.timeout_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::RunCommandRequest;
    use std::path::PathBuf;

    #[test]
    fn rejects_empty_argv() {
        assert!(RunCommandRequest::new(PathBuf::from("/tmp/repo"), vec![]).is_err());
    }

    #[test]
    fn stores_timeout() {
        let request = RunCommandRequest::new(
            PathBuf::from("/tmp/repo"),
            vec!["cargo".to_string(), "test".to_string()],
        )
        .unwrap()
        .with_timeout_seconds(120);
        assert_eq!(request.timeout_seconds(), 120);
    }
}
