use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::AllowedPaths;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementChangeRequest {
    worktree_path: PathBuf,
    task: String,
    allowed_paths: Option<AllowedPaths>,
    timeout_seconds: u32,
    max_turns: usize,
}

impl ImplementChangeRequest {
    pub fn new(worktree_path: PathBuf, task: String) -> Self {
        Self {
            worktree_path,
            task,
            allowed_paths: None,
            timeout_seconds: 900,
            max_turns: 8,
        }
    }

    pub fn with_policy(mut self, allowed_paths: AllowedPaths, timeout_seconds: u32) -> Self {
        self.allowed_paths = Some(allowed_paths);
        self.timeout_seconds = timeout_seconds;
        self
    }

    pub fn with_max_turns(mut self, max_turns: usize) -> Self {
        self.max_turns = max_turns;
        self
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn task(&self) -> &str {
        self.task.as_str()
    }

    pub fn allowed_paths(&self) -> Result<&AllowedPaths, String> {
        self.allowed_paths
            .as_ref()
            .ok_or("implement request missing allowed paths".to_string())
    }

    pub fn timeout_seconds(&self) -> u32 {
        self.timeout_seconds
    }

    pub fn max_turns(&self) -> usize {
        self.max_turns
    }
}

#[cfg(test)]
mod tests {
    use super::ImplementChangeRequest;
    use rack_ai_domain::AllowedPath;
    use rack_ai_domain::AllowedPaths;
    use std::path::PathBuf;

    #[test]
    fn stores_task_and_timeout() {
        let request =
            ImplementChangeRequest::new(PathBuf::from("/tmp/repo"), "Add a feature.".to_string())
                .with_policy(
                    AllowedPaths::new(vec![AllowedPath::new("src".to_string()).unwrap()]).unwrap(),
                    120,
                );
        assert_eq!(request.timeout_seconds(), 120);
        assert_eq!(request.task(), "Add a feature.");
    }
}
