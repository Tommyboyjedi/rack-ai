use std::fs;
use std::path::PathBuf;
use std::process::Command;

use rack_ai_application::TaskExecution;
use rack_ai_application::TaskExecutionRequest;
use rack_ai_application::TaskExecutor;

pub struct CliRackTaskExecutor {
    repo_root: PathBuf,
    state_root: PathBuf,
}

impl CliRackTaskExecutor {
    pub fn new(repo_root: PathBuf, state_root: PathBuf) -> Self {
        Self {
            repo_root,
            state_root,
        }
    }
}

impl TaskExecutor for CliRackTaskExecutor {
    fn execute(&self, request: &TaskExecutionRequest) -> Result<TaskExecution, String> {
        let execution_path = self.prepare_execution_path(request)?;
        let output = Command::new(self.repo_root.join("bin/rack-task"))
            .arg("--emit-json")
            .arg(&execution_path)
            .output()
            .map_err(|error| error.to_string())?;
        self.cleanup_execution_path(request, &execution_path);
        let result_path = self.write_result(request.task_id(), &output.stdout)?;
        if output.status.success() {
            return Ok(TaskExecution::success(Some(result_path)));
        }
        let last_error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let error_text = if last_error.is_empty() {
            "rack-task failed".to_string()
        } else {
            last_error
        };
        Ok(TaskExecution::failure(error_text, Some(result_path)))
    }
}

impl CliRackTaskExecutor {
    fn prepare_execution_path(&self, request: &TaskExecutionRequest) -> Result<PathBuf, String> {
        if let Some(execution_spec_json) = request.execution_spec_json() {
            let path = self
                .state_root
                .join("state/queue/running")
                .join(format!("{}--exec.json", request.task_id()));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(&path, format!("{execution_spec_json}\n"))
                .map_err(|error| error.to_string())?;
            return Ok(path);
        }
        Ok(PathBuf::from(request.source_spec_path()))
    }

    fn cleanup_execution_path(&self, request: &TaskExecutionRequest, execution_path: &PathBuf) {
        if request.execution_spec_json().is_some() {
            let _ = fs::remove_file(execution_path);
        }
    }

    fn write_result(&self, task_id: &str, stdout: &[u8]) -> Result<String, String> {
        let history_dir = self.state_root.join("state/queue/history");
        fs::create_dir_all(&history_dir).map_err(|error| error.to_string())?;
        let path = history_dir.join(format!("{task_id}.result.json"));
        fs::write(&path, stdout).map_err(|error| error.to_string())?;
        Ok(path.to_string_lossy().to_string())
    }
}
