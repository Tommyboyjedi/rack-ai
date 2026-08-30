use std::sync::Arc;

use rack_ai_application::ContainerLifecycleObserver;
use rack_ai_application::ExecutorConfig;
use rack_ai_application::ReadFileRequest;
use rack_ai_application::RunCommandRequest;
use rack_ai_application::WorkspaceExecutionResult;
use rack_ai_application::WorkspaceExecutor;
use rack_ai_application::WriteFileRequest;

use crate::HostWorkspaceExecutor;
use crate::PodmanWorkspaceExecutor;

pub enum ConfiguredWorkspaceExecutor {
    Host(HostWorkspaceExecutor),
    Podman(PodmanWorkspaceExecutor),
}

impl ConfiguredWorkspaceExecutor {
    pub fn new(config: ExecutorConfig) -> Result<Self, String> {
        match config.backend() {
            "host" => Ok(Self::Host(HostWorkspaceExecutor::new())),
            "podman" => Ok(Self::Podman(PodmanWorkspaceExecutor::new(config))),
            value => Err(format!("unsupported executor backend: {value}")),
        }
    }

    pub fn with_observer(
        config: ExecutorConfig,
        observer: Arc<dyn ContainerLifecycleObserver>,
    ) -> Result<Self, String> {
        match config.backend() {
            "host" => Ok(Self::Host(HostWorkspaceExecutor::new())),
            "podman" => Ok(Self::Podman(
                PodmanWorkspaceExecutor::new(config).with_observer(observer),
            )),
            value => Err(format!("unsupported executor backend: {value}")),
        }
    }
}

impl WorkspaceExecutor for ConfiguredWorkspaceExecutor {
    fn write_file(&self, request: &WriteFileRequest) -> Result<WorkspaceExecutionResult, String> {
        match self {
            Self::Host(executor) => executor.write_file(request),
            Self::Podman(executor) => executor.write_file(request),
        }
    }

    fn read_file(&self, request: &ReadFileRequest) -> Result<WorkspaceExecutionResult, String> {
        match self {
            Self::Host(executor) => executor.read_file(request),
            Self::Podman(executor) => executor.read_file(request),
        }
    }

    fn run_command(&self, request: &RunCommandRequest) -> Result<WorkspaceExecutionResult, String> {
        match self {
            Self::Host(executor) => executor.run_command(request),
            Self::Podman(executor) => executor.run_command(request),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConfiguredWorkspaceExecutor;
    use rack_ai_application::ExecutorConfig;

    #[test]
    fn selects_host_backend() {
        let executor = ConfiguredWorkspaceExecutor::new(ExecutorConfig::host()).unwrap();
        assert!(matches!(executor, ConfiguredWorkspaceExecutor::Host(_)));
    }

    #[test]
    fn selects_podman_backend() {
        let executor = ConfiguredWorkspaceExecutor::new(
            ExecutorConfig::podman("docker.io/library/rust:bookworm".to_string()).unwrap(),
        )
        .unwrap();
        assert!(matches!(executor, ConfiguredWorkspaceExecutor::Podman(_)));
    }
}
