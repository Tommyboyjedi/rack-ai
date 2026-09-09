use std::path::Path;
use std::path::PathBuf;

use rack_ai_application::EnvironmentResourceMount;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PodmanInvocation {
    image: String,
    worktree_path: PathBuf,
    workspace_mount: String,
    timeout_seconds: u32,
    memory: String,
    pids_limit: u32,
    argv: Vec<String>,
    stdin: Option<String>,
    cidfile: Option<PathBuf>,
    environment_resources: Vec<EnvironmentResourceMount>,
}

impl PodmanInvocation {
    pub fn new(image: String, worktree_path: PathBuf) -> Result<Self, String> {
        if image.trim().is_empty() {
            return Err("executor image cannot be empty".to_string());
        }
        if !worktree_path.is_absolute() {
            return Err("worktree path must be absolute".to_string());
        }
        Ok(Self {
            image,
            worktree_path,
            workspace_mount: "/workspace".to_string(),
            timeout_seconds: 30,
            memory: "2g".to_string(),
            pids_limit: 256,
            argv: Vec::new(),
            stdin: None,
            cidfile: None,
            environment_resources: Vec::new(),
        })
    }

    pub fn with_workspace_mount(mut self, workspace_mount: String) -> Self {
        self.workspace_mount = workspace_mount;
        self
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u32) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    pub fn with_memory(mut self, memory: String) -> Self {
        self.memory = memory;
        self
    }

    pub fn with_pids_limit(mut self, pids_limit: u32) -> Self {
        self.pids_limit = pids_limit;
        self
    }

    pub fn with_argv(mut self, argv: Vec<String>) -> Self {
        self.argv = argv;
        self
    }

    pub fn with_stdin(mut self, stdin: Option<String>) -> Self {
        self.stdin = stdin;
        self
    }

    pub fn with_cidfile(mut self, cidfile: PathBuf) -> Self {
        self.cidfile = Some(cidfile);
        self
    }

    pub fn with_environment_resources(
        mut self,
        environment_resources: Vec<EnvironmentResourceMount>,
    ) -> Self {
        self.environment_resources = environment_resources;
        self
    }

    pub fn image(&self) -> &str {
        self.image.as_str()
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn workspace_mount(&self) -> &str {
        self.workspace_mount.as_str()
    }

    pub fn timeout_seconds(&self) -> u32 {
        self.timeout_seconds
    }

    pub fn memory(&self) -> &str {
        self.memory.as_str()
    }

    pub fn pids_limit(&self) -> u32 {
        self.pids_limit
    }

    pub fn argv(&self) -> &[String] {
        self.argv.as_slice()
    }

    pub fn stdin(&self) -> Option<&String> {
        self.stdin.as_ref()
    }

    pub fn cidfile(&self) -> Option<&Path> {
        self.cidfile.as_deref()
    }

    pub fn environment_resources(&self) -> &[EnvironmentResourceMount] {
        self.environment_resources.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::PodmanInvocation;
    use std::path::PathBuf;

    #[test]
    fn requires_absolute_worktree() {
        assert!(PodmanInvocation::new("rust:bookworm".to_string(), PathBuf::from("repo")).is_err());
    }
}
