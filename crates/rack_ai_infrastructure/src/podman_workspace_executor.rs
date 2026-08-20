use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use rack_ai_application::CommandEvidence;
use rack_ai_application::ExecutorConfig;
use rack_ai_application::ReadFileRequest;
use rack_ai_application::RunCommandRequest;
use rack_ai_application::WorkspaceExecutionResult;
use rack_ai_application::WorkspaceExecutor;
use rack_ai_application::WriteFileRequest;

use crate::PodmanAvailability;
use crate::PodmanInvocation;
use crate::PodmanRunPlan;

pub struct PodmanWorkspaceExecutor {
    config: ExecutorConfig,
    command: String,
}

impl PodmanWorkspaceExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self::new_with_command(config, "podman".to_string())
    }

    pub fn new_with_command(config: ExecutorConfig, command: String) -> Self {
        Self { config, command }
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
    ) -> Result<WorkspaceExecutionResult, String> {
        if !worktree_path.is_dir() {
            return Err(format!(
                "worktree does not exist: {}",
                worktree_path.display()
            ));
        }
        PodmanAvailability::ensure_command(self.command.as_str())?;
        let invocation =
            PodmanInvocation::new(self.config.image().to_string(), worktree_path.to_path_buf())?
                .with_workspace_mount(self.config.workspace_mount().to_string())
                .with_memory(self.config.memory().to_string())
                .with_pids_limit(self.config.pids_limit())
                .with_timeout_seconds(timeout_seconds)
                .with_argv(argv.clone())
                .with_stdin(stdin.clone());
        let plan = PodmanRunPlan::from_invocation(&invocation)?;
        let mut command = Command::new(self.command.as_str());
        command.args(plan.arguments());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command.spawn().map_err(|error| map_spawn_error(error))?;
        if let Some(payload) = stdin {
            if let Some(mut handle) = child.stdin.take() {
                handle
                    .write_all(payload.as_bytes())
                    .map_err(|error| error.to_string())?;
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let timed_out = stderr.to_lowercase().contains("timed out")
            || stderr.to_lowercase().contains("timeout");
        let evidence = CommandEvidence::new(argv, output.status.code().unwrap_or(1))
            .with_stdout(stdout)
            .with_stderr(stderr)
            .with_timed_out(timed_out);
        Ok(WorkspaceExecutionResult::new(evidence))
    }
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
        for item in [error, read_error, run_error] {
            assert!(
                item.contains("podman is not available")
                    || item.contains("worktree does not exist"),
                "{item}"
            );
        }
    }

    fn existing_worktree() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-podman-ws-{nanos}"));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}
