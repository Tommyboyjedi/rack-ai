use std::fs;
use std::path::PathBuf;
use std::process::Command;

use rack_ai_application::TaskExecution;
use rack_ai_application::TaskExecutionRequest;
use rack_ai_application::TaskExecutor;

pub struct PythonRackTaskExecutor {
    repo_root: PathBuf,
    state_root: PathBuf,
}

impl PythonRackTaskExecutor {
    pub fn new(repo_root: PathBuf, state_root: PathBuf) -> Self {
        Self {
            repo_root,
            state_root,
        }
    }
}

impl TaskExecutor for PythonRackTaskExecutor {
    fn execute(&self, request: &TaskExecutionRequest) -> Result<TaskExecution, String> {
        let execution_path = self.prepare_execution_path(request)?;
        let output = Command::new(self.repo_root.join("bin/rack-task"))
            .arg("--emit-json")
            .arg(&execution_path)
            .output()
            .map_err(|error| error.to_string())?;
        self.cleanup_execution_path(request, &execution_path);
        if output.status.success() {
            return Ok(TaskExecution::success());
        }
        Ok(TaskExecution::failure())
    }
}

impl PythonRackTaskExecutor {
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
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::TaskExecutionRequest;
    use rack_ai_application::TaskExecutor;

    use super::PythonRackTaskExecutor;

    #[test]
    fn runs_script_and_reports_success() {
        let repo_root = temp_root("repo");
        let state_root = temp_root("state");
        let bin_dir = repo_root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(state_root.join("state/queue/running")).unwrap();
        let script = bin_dir.join("rack-task");
        fs::write(&script, "#!/usr/bin/env bash\nexit 0\n").unwrap();
        let _ = std::process::Command::new("chmod")
            .arg("+x")
            .arg(&script)
            .status();
        let spec = state_root.join("spec.json");
        fs::write(&spec, "{}").unwrap();
        let executor = PythonRackTaskExecutor::new(repo_root, state_root);
        let outcome = executor
            .execute(&TaskExecutionRequest::new(
                "task".to_string(),
                spec.to_str().unwrap().to_string(),
            ))
            .unwrap();
        assert!(outcome.was_successful());
    }

    #[test]
    fn writes_temporary_execution_spec_for_dag_nodes_into_state_root() {
        let repo_root = temp_root("repo");
        let state_root = temp_root("state");
        let bin_dir = repo_root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(state_root.join("state/queue/running")).unwrap();
        let script = bin_dir.join("rack-task");
        fs::write(&script, "#!/usr/bin/env bash\ntest -f \"$2\"\nexit 0\n").unwrap();
        let _ = std::process::Command::new("chmod")
            .arg("+x")
            .arg(&script)
            .status();
        let executor = PythonRackTaskExecutor::new(repo_root, state_root.clone());
        executor
            .execute(
                &TaskExecutionRequest::new("task".to_string(), "/tmp/spec.json".to_string())
                    .with_execution_spec_json("{}".to_string()),
            )
            .unwrap();
        assert!(
            !state_root
                .join("state/queue/running/task--exec.json")
                .exists()
        );
    }

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-python-executor-{label}-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
