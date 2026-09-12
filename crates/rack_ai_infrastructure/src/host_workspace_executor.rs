use std::fs;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use rack_ai_application::CommandEvidence;
use rack_ai_application::ReadFileRequest;
use rack_ai_application::RunCommandRequest;
use rack_ai_application::WorkspaceExecutionResult;
use rack_ai_application::WorkspaceExecutor;
use rack_ai_application::WriteFileRequest;

use crate::WaitOutcome;
use crate::WallClockWait;

pub struct HostWorkspaceExecutor;

impl HostWorkspaceExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl WorkspaceExecutor for HostWorkspaceExecutor {
    fn write_file(&self, request: &WriteFileRequest) -> Result<WorkspaceExecutionResult, String> {
        ensure_worktree(request.worktree_path())?;
        let path = request.worktree_path().join(request.path().relative());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, request.content()).map_err(|error| error.to_string())?;
        Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
            vec!["write".to_string(), request.path().relative().to_string()],
            0,
        )))
    }

    fn read_file(&self, request: &ReadFileRequest) -> Result<WorkspaceExecutionResult, String> {
        ensure_worktree(request.worktree_path())?;
        let path = request.worktree_path().join(request.path().relative());
        let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let ranged = read_range(&content, request.start_line(), request.limit());
        Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
            vec!["read".to_string(), request.path().relative().to_string()],
            0,
        ))
        .with_content(ranged))
    }

    fn run_command(&self, request: &RunCommandRequest) -> Result<WorkspaceExecutionResult, String> {
        ensure_worktree(request.worktree_path())?;
        let mut command = Command::new(&request.argv()[0]);
        command.args(&request.argv()[1..]);
        command.current_dir(request.worktree_path());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|error| map_spawn_error(&request.argv()[0], error))?;
        match WallClockWait::child_output(child, request.timeout_seconds())? {
            WaitOutcome::Completed(output) => {
                let evidence = CommandEvidence::new(
                    request.argv().to_vec(),
                    output.status.code().unwrap_or(1),
                )
                .with_stdout(String::from_utf8_lossy(&output.stdout).to_string())
                .with_stderr(String::from_utf8_lossy(&output.stderr).to_string());
                Ok(WorkspaceExecutionResult::new(evidence))
            }
            WaitOutcome::TimedOut => {
                let evidence = CommandEvidence::new(request.argv().to_vec(), 124)
                    .with_stderr(format!(
                        "workspace command exceeded wall-clock timeout of {}s",
                        request.timeout_seconds()
                    ))
                    .with_timed_out(true);
                Ok(WorkspaceExecutionResult::new(evidence))
            }
        }
    }
}

fn ensure_worktree(worktree_path: &Path) -> Result<(), String> {
    if worktree_path.is_dir() {
        Ok(())
    } else {
        Err(format!(
            "worktree does not exist: {}",
            worktree_path.display()
        ))
    }
}

fn read_range(content: &str, start_line: u64, limit: u64) -> String {
    let start = start_line.saturating_sub(1) as usize;
    let end = start.saturating_add(limit as usize);
    content
        .split_inclusive('\n')
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn map_spawn_error(program: &str, error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        format!("workspace command executable is not available: {program}")
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::HostWorkspaceExecutor;
    use rack_ai_application::ReadFileRequest;
    use rack_ai_application::RunCommandRequest;
    use rack_ai_application::WorkspaceExecutor;
    use rack_ai_application::WorkspacePath;
    use rack_ai_application::WriteFileRequest;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-host-executor-{nanos}"));
        fs::create_dir_all(root.join("src")).unwrap();
        root
    }

    #[test]
    fn writes_and_reads_file_inside_worktree() {
        let worktree = fixture();
        let executor = HostWorkspaceExecutor::new();
        executor
            .write_file(
                &WriteFileRequest::new(
                    worktree.clone(),
                    WorkspacePath::parse("src/lib.rs").unwrap(),
                )
                .with_content("line1\nline2\nline3\n".to_string()),
            )
            .unwrap();
        let content = executor
            .read_file(
                &ReadFileRequest::new(worktree, WorkspacePath::parse("src/lib.rs").unwrap())
                    .with_range(2, 1),
            )
            .unwrap();
        assert_eq!(content.content(), "line2\n");
    }

    #[test]
    fn runs_command_in_requested_worktree_and_captures_output() {
        let worktree = fixture();
        let executor = HostWorkspaceExecutor::new();
        let result = executor
            .run_command(
                &RunCommandRequest::new(
                    worktree.clone(),
                    vec![
                        "python3".to_string(),
                        "-c".to_string(),
                        "import pathlib, sys; print(pathlib.Path.cwd()); print(\"stderr\", file=sys.stderr); raise SystemExit(7)".to_string(),
                    ],
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(result.evidence().exit_code(), 7);
        assert!(
            result
                .evidence()
                .stdout()
                .contains(worktree.to_string_lossy().as_ref())
        );
        assert!(result.evidence().stderr().contains("stderr"));
    }

    #[test]
    fn executes_absolute_executable_path() {
        let worktree = fixture();
        let executor = HostWorkspaceExecutor::new();
        let result = executor
            .run_command(
                &RunCommandRequest::new(worktree.clone(), vec!["/bin/pwd".to_string()]).unwrap(),
            )
            .unwrap();
        assert!(result.evidence().succeeded());
        assert_eq!(
            result.evidence().stdout().trim(),
            worktree.to_string_lossy()
        );
    }

    #[test]
    fn times_out_hung_command() {
        let worktree = fixture();
        let executor = HostWorkspaceExecutor::new();
        let result = executor
            .run_command(
                &RunCommandRequest::new(worktree, vec!["sleep".to_string(), "20".to_string()])
                    .unwrap()
                    .with_timeout_seconds(1),
            )
            .unwrap();
        assert!(result.evidence().timed_out());
        assert_eq!(result.evidence().exit_code(), 124);
    }

    #[test]
    fn reports_missing_executable_cleanly() {
        let worktree = fixture();
        let executor = HostWorkspaceExecutor::new();
        let error = executor
            .run_command(
                &RunCommandRequest::new(
                    worktree,
                    vec!["definitely-not-a-real-command".to_string()],
                )
                .unwrap(),
            )
            .unwrap_err();
        assert!(error.contains("workspace command executable is not available"));
    }
}
