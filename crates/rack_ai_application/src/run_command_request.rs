use std::path::Path;
use std::path::PathBuf;

use crate::EnvironmentResourceMount;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunCommandRequest {
    worktree_path: PathBuf,
    argv: Vec<String>,
    timeout_seconds: u32,
    environment_resources: Vec<EnvironmentResourceMount>,
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
            environment_resources: Vec::new(),
        })
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u32) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    pub fn with_environment_resources(
        mut self,
        environment_resources: Vec<EnvironmentResourceMount>,
    ) -> Self {
        self.environment_resources = environment_resources;
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

    pub fn environment_resources(&self) -> &[EnvironmentResourceMount] {
        self.environment_resources.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::RunCommandRequest;
    use crate::EnvironmentResourceMount;
    use std::path::PathBuf;

    #[test]
    fn rejects_empty_argv() {
        assert!(RunCommandRequest::new(PathBuf::from("/tmp/repo"), vec![]).is_err());
    }

    #[test]
    fn stores_timeout_and_environment_resources() {
        let request = RunCommandRequest::new(
            PathBuf::from("/tmp/repo"),
            vec!["cargo".to_string(), "test".to_string()],
        )
        .unwrap()
        .with_timeout_seconds(120)
        .with_environment_resources(vec![
            EnvironmentResourceMount::same_path(PathBuf::from("/srv/runtime/.venv")).unwrap(),
        ]);
        assert_eq!(request.timeout_seconds(), 120);
        assert_eq!(request.environment_resources().len(), 1);
    }
}
