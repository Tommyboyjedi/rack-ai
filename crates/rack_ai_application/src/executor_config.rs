#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutorConfig {
    backend: String,
    image: String,
    workspace_mount: String,
    memory: String,
    pids_limit: u32,
}

impl ExecutorConfig {
    pub fn podman(image: String) -> Result<Self, String> {
        if image.trim().is_empty() {
            return Err("executor image cannot be empty".to_string());
        }
        Ok(Self {
            backend: "podman".to_string(),
            image,
            workspace_mount: "/workspace".to_string(),
            memory: "2g".to_string(),
            pids_limit: 256,
        })
    }

    pub fn with_workspace_mount(mut self, workspace_mount: String) -> Self {
        self.workspace_mount = workspace_mount;
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

    pub fn backend(&self) -> &str {
        self.backend.as_str()
    }

    pub fn image(&self) -> &str {
        self.image.as_str()
    }

    pub fn workspace_mount(&self) -> &str {
        self.workspace_mount.as_str()
    }

    pub fn memory(&self) -> &str {
        self.memory.as_str()
    }

    pub fn pids_limit(&self) -> u32 {
        self.pids_limit
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutorConfig;

    #[test]
    fn builds_podman_defaults() {
        let config = ExecutorConfig::podman("docker.io/library/rust:bookworm".to_string()).unwrap();
        assert_eq!(config.backend(), "podman");
        assert_eq!(config.workspace_mount(), "/workspace");
        assert_eq!(config.pids_limit(), 256);
    }
}
