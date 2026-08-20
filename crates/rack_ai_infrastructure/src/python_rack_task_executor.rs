use std::path::PathBuf;
use std::process::Command;

use rack_ai_application::QueuedTask;
use rack_ai_application::TaskExecution;
use rack_ai_application::TaskExecutor;

pub struct PythonRackTaskExecutor {
    root: PathBuf,
}

impl PythonRackTaskExecutor {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl TaskExecutor for PythonRackTaskExecutor {
    fn execute(&self, task: &QueuedTask) -> Result<TaskExecution, String> {
        let output = Command::new(self.root.join("bin/rack-task"))
            .arg("--emit-json")
            .arg(task.spec_path())
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            return Ok(TaskExecution::success());
        }
        Ok(TaskExecution::failure())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::QueuedTask;
    use rack_ai_application::TaskExecutor;

    use super::PythonRackTaskExecutor;

    #[test]
    fn runs_script_and_reports_success() {
        let root = temp_root();
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let script = bin_dir.join("rack-task");
        fs::write(&script, "#!/usr/bin/env bash\nexit 0\n").unwrap();
        let _ = std::process::Command::new("chmod")
            .arg("+x")
            .arg(&script)
            .status();
        let spec = root.join("spec.json");
        fs::write(&spec, "{}").unwrap();
        let executor = PythonRackTaskExecutor::new(root);
        let outcome = executor
            .execute(&QueuedTask::new(
                "task".to_string(),
                spec.to_str().unwrap().to_string(),
            ))
            .unwrap();
        assert!(outcome.was_successful());
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-python-executor-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
