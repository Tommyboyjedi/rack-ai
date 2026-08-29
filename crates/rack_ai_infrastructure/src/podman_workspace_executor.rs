use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rack_ai_application::CommandEvidence;
use rack_ai_application::ContainerLifecycleObserver;
use rack_ai_application::EnvironmentResourceMount;
use rack_ai_application::ExecutorConfig;
use rack_ai_application::ReadFileRequest;
use rack_ai_application::RunCommandRequest;
use rack_ai_application::WorkspaceExecutionResult;
use rack_ai_application::WorkspaceExecutor;
use rack_ai_application::WriteFileRequest;

use crate::PodmanAvailability;
use crate::PodmanContainerCleanup;
use crate::PodmanInvocation;
use crate::PodmanRunPlan;
use crate::WaitOutcome;
use crate::WallClockWait;

pub struct PodmanWorkspaceExecutor {
    config: ExecutorConfig,
    command: String,
    observer: Option<Arc<dyn ContainerLifecycleObserver>>,
}

impl PodmanWorkspaceExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self::new_with_command(config, "podman".to_string())
    }

    pub fn new_with_command(config: ExecutorConfig, command: String) -> Self {
        Self {
            config,
            command,
            observer: None,
        }
    }

    pub fn with_observer(mut self, observer: Arc<dyn ContainerLifecycleObserver>) -> Self {
        self.observer = Some(observer);
        self
    }
}

impl WorkspaceExecutor for PodmanWorkspaceExecutor {
    fn write_file(&self, request: &WriteFileRequest) -> Result<WorkspaceExecutionResult, String> {
        self.execute_invocation(
            request.worktree_path(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "mkdir -p -- \"$(dirname -- \"$1\")\" && cat > \"$1\"".to_string(),
                "write".to_string(),
                request.path().container_path(),
            ],
            Some(request.content().to_string()),
            request.timeout_seconds(),
            &[],
        )
    }

    fn read_file(&self, request: &ReadFileRequest) -> Result<WorkspaceExecutionResult, String> {
        let end = request
            .start_line()
            .saturating_add(request.limit())
            .saturating_sub(1);
        let result = self.execute_invocation(
            request.worktree_path(),
            vec![
                "/bin/sed".to_string(),
                "-n".to_string(),
                format!("{},{}p", request.start_line(), end),
                request.path().container_path(),
            ],
            None,
            request.timeout_seconds(),
            &[],
        )?;
        let content = result.evidence().stdout().to_string();
        Ok(result.with_content(content))
    }

    fn run_command(&self, request: &RunCommandRequest) -> Result<WorkspaceExecutionResult, String> {
        self.execute_invocation(
            request.worktree_path(),
            request.argv().to_vec(),
            None,
            request.timeout_seconds(),
            request.environment_resources(),
        )
    }
}

impl PodmanWorkspaceExecutor {
    fn execute_invocation(
        &self,
        worktree_path: &Path,
        argv: Vec<String>,
        stdin: Option<String>,
        timeout_seconds: u32,
        environment_resources: &[EnvironmentResourceMount],
    ) -> Result<WorkspaceExecutionResult, String> {
        if !worktree_path.is_dir() {
            return Err(format!(
                "worktree does not exist: {}",
                worktree_path.display()
            ));
        }
        PodmanAvailability::ensure_command(self.command.as_str())?;
        PodmanAvailability::ensure_image(self.command.as_str(), self.config.image())?;
        let cidfile = unique_cidfile();
        let cleanup = PodmanContainerCleanup::new(self.command.clone(), cidfile.clone());
        let invocation =
            PodmanInvocation::new(self.config.image().to_string(), worktree_path.to_path_buf())?
                .with_workspace_mount(self.config.workspace_mount().to_string())
                .with_memory(self.config.memory().to_string())
                .with_pids_limit(self.config.pids_limit())
                .with_timeout_seconds(timeout_seconds)
                .with_argv(argv.clone())
                .with_stdin(stdin.clone())
                .with_cidfile(cidfile)
                .with_environment_resources(environment_resources.to_vec());
        let plan = PodmanRunPlan::from_invocation(&invocation)?;
        let mut command = Command::new(self.command.as_str());
        command.args(plan.arguments());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(cleanup.cidfile());
                return Err(map_spawn_error(error));
            }
        };
        let tracked_container_id =
            match self.track_container_start(cleanup.cidfile(), timeout_seconds) {
                Ok(value) => value,
                Err(error) => {
                    cleanup.stop_and_remove();
                    let _ = self.track_container_finish();
                    return Err(error);
                }
            };
        if let Some(payload) = stdin {
            if let Some(mut handle) = child.stdin.take() {
                if let Err(error) = handle.write_all(payload.as_bytes()) {
                    cleanup.stop_and_remove();
                    let _ = self.track_container_finish();
                    return Err(error.to_string());
                }
            }
        }
        let wait = WallClockWait::child_output(child, timeout_seconds)?;
        let finish_result = self.track_container_finish();
        match wait {
            WaitOutcome::Completed(output) => {
                let _ = fs::remove_file(cleanup.cidfile());
                finish_result?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let evidence = CommandEvidence::new(argv, output.status.code().unwrap_or(1))
                    .with_stdout(stdout)
                    .with_stderr(stderr);
                Ok(WorkspaceExecutionResult::new(evidence))
            }
            WaitOutcome::TimedOut => {
                cleanup.stop_and_remove();
                finish_result?;
                let stderr = match tracked_container_id {
                    Some(id) => format!(
                        "workspace command exceeded wall-clock timeout of {timeout_seconds}s (container {id})"
                    ),
                    None => format!(
                        "workspace command exceeded wall-clock timeout of {timeout_seconds}s"
                    ),
                };
                let evidence = CommandEvidence::new(argv, 124)
                    .with_stderr(stderr)
                    .with_timed_out(true);
                Ok(WorkspaceExecutionResult::new(evidence))
            }
        }
    }

    fn track_container_start(
        &self,
        cidfile: &Path,
        timeout_seconds: u32,
    ) -> Result<Option<String>, String> {
        let Some(observer) = self.observer.as_ref() else {
            return Ok(None);
        };
        let container_id = wait_for_container_id(cidfile, timeout_seconds);
        if let Some(container_id) = container_id.as_deref() {
            observer.container_started(container_id)?;
        }
        Ok(container_id)
    }

    fn track_container_finish(&self) -> Result<(), String> {
        let Some(observer) = self.observer.as_ref() else {
            return Ok(());
        };
        observer.container_finished()
    }
}

fn wait_for_container_id(cidfile: &Path, timeout_seconds: u32) -> Option<String> {
    let max_wait = Duration::from_secs(timeout_seconds.min(5) as u64);
    let start = SystemTime::now();
    loop {
        if let Ok(text) = fs::read_to_string(cidfile) {
            let id = text.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        if start.elapsed().unwrap_or_default() >= max_wait {
            return None;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn unique_cidfile() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("rack-ai-{nanos}.cid"))
}

fn map_spawn_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "podman is not available; rootless Podman is required for external-repository command execution"
            .to_string()
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::PodmanWorkspaceExecutor;
    use rack_ai_application::ExecutorConfig;
    use rack_ai_application::ReadFileRequest;
    use rack_ai_application::RunCommandRequest;
    use rack_ai_application::WorkspaceExecutor;
    use rack_ai_application::WorkspacePath;
    use rack_ai_application::WriteFileRequest;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn rejects_missing_worktree_before_podman() {
        let executor = PodmanWorkspaceExecutor::new(
            ExecutorConfig::podman("rust:bookworm".to_string()).unwrap(),
        );
        let missing = PathBuf::from("/tmp/rack-ai-missing-worktree-does-not-exist");
        let error = executor
            .run_command(&RunCommandRequest::new(missing, vec!["true".to_string()]).unwrap())
            .unwrap_err();
        assert!(error.contains("worktree does not exist"));
    }

    #[test]
    fn fails_closed_when_podman_is_unavailable() {
        let executor = PodmanWorkspaceExecutor::new_with_command(
            ExecutorConfig::podman("rust:bookworm".to_string()).unwrap(),
            "__definitely_missing_podman_binary__".to_string(),
        );
        let worktree = existing_worktree();
        let error = executor
            .write_file(
                &WriteFileRequest::new(
                    worktree.clone(),
                    WorkspacePath::parse("src/lib.rs").unwrap(),
                )
                .with_content("fn main() {}".to_string()),
            )
            .unwrap_err();
        let read_error = executor
            .read_file(&ReadFileRequest::new(
                worktree.clone(),
                WorkspacePath::parse("src/lib.rs").unwrap(),
            ))
            .unwrap_err();
        let run_error = executor
            .run_command(&RunCommandRequest::new(worktree, vec!["true".to_string()]).unwrap())
            .unwrap_err();
        assert!(error.contains("podman is not available"));
        assert!(read_error.contains("podman is not available"));
        assert!(run_error.contains("podman is not available"));
    }

    fn existing_worktree() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rack-ai-existing-worktree-{nanos}"));
        std::fs::create_dir_all(path.join("src")).unwrap();
        path
    }
}
