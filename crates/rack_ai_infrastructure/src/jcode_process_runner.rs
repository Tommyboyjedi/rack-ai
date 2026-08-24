use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rack_ai_application::ImplementWorkerRuntime;

#[derive(Debug)]
pub struct JCodeProcessOutput {
    stdout: String,
    stderr: String,
}

impl JCodeProcessOutput {
    pub fn stdout(&self) -> &str {
        self.stdout.as_str()
    }

    pub fn stderr(&self) -> &str {
        self.stderr.as_str()
    }
}

pub struct JCodeProcessRunner;

impl JCodeProcessRunner {
    pub fn run(
        runtime: &ImplementWorkerRuntime,
        task: &str,
        workdir: &Path,
        timeout_seconds: u32,
    ) -> Result<JCodeProcessOutput, String> {
        let mut command = Command::new(runtime.entrypoint());
        command
            .arg("--no-update")
            .arg("--no-selfdev")
            .arg("--quiet")
            .arg("--trace")
            .arg("--provider-profile")
            .arg(runtime.provider_profile())
            .arg("--model")
            .arg(runtime.api_model_id());
        if let Some(tool_profile) = runtime.tool_profile() {
            command.arg("--tool-profile").arg(tool_profile);
        }
        command
            .arg("-C")
            .arg(workdir)
            .arg("run")
            .arg(task)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let timeout_seconds = timeout_seconds.max(1);
        let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_seconds));
        loop {
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                let mut stdout = String::new();
                let mut stderr = String::new();
                child
                    .stdout
                    .take()
                    .ok_or_else(|| "missing JCode stdout pipe".to_string())?
                    .read_to_string(&mut stdout)
                    .map_err(|error| error.to_string())?;
                child
                    .stderr
                    .take()
                    .ok_or_else(|| "missing JCode stderr pipe".to_string())?
                    .read_to_string(&mut stderr)
                    .map_err(|error| error.to_string())?;
                if !status.success() {
                    return Err(format!(
                        "jcode exited unsuccessfully for worker {}: {}",
                        runtime.worker_id(),
                        stderr.trim()
                    ));
                }
                return Ok(JCodeProcessOutput { stdout, stderr });
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "jcode wall-clock timeout exceeded for worker {} after {} seconds",
                    runtime.worker_id(),
                    timeout_seconds
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::ImplementWorkerRuntime;

    use super::JCodeProcessRunner;

    #[test]
    fn passes_expected_arguments_and_collects_trace_output() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let args_log = root.join("args.log");
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                concat!(
                    "#!/usr/bin/env bash\n",
                    "set -euo pipefail\n",
                    "printf '%s\\n' \"$@\" > '{}'\n",
                    "printf 'trace-line\\n' >&2\n",
                    "printf 'COMPLETE\\n'\n"
                ),
                args_log.display()
            )
            .as_str(),
        );
        let runtime = ImplementWorkerRuntime::new(
            "local-coder".to_string(),
            script.display().to_string(),
            "local-coder".to_string(),
            "local-coder".to_string(),
            "http://127.0.0.1:8018/v1".to_string(),
        )
        .with_tool_profile(Some("minimal".to_string()));

        let output = JCodeProcessRunner::run(&runtime, "fix the file", &workdir, 5).unwrap();

        assert_eq!(output.stdout(), "COMPLETE\n");
        assert_eq!(output.stderr(), "trace-line\n");
        let args = fs::read_to_string(args_log).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        assert_eq!(
            lines,
            vec![
                "--no-update",
                "--no-selfdev",
                "--quiet",
                "--trace",
                "--provider-profile",
                "local-coder",
                "--model",
                "local-coder",
                "--tool-profile",
                "minimal",
                "-C",
                workdir.to_str().unwrap(),
                "run",
                "fix the file",
            ]
        );
    }

    #[test]
    fn omits_tool_profile_when_worker_does_not_require_one() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let args_log = root.join("args.log");
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                concat!(
                    "#!/usr/bin/env bash\n",
                    "set -euo pipefail\n",
                    "printf '%s\\n' \"$@\" > '{}'\n",
                    "printf 'COMPLETE\\n'\n"
                ),
                args_log.display()
            )
            .as_str(),
        );
        let runtime = ImplementWorkerRuntime::new(
            "local-primary".to_string(),
            script.display().to_string(),
            "local-primary".to_string(),
            "local-primary".to_string(),
            "http://127.0.0.1:8017/v1".to_string(),
        );

        JCodeProcessRunner::run(&runtime, "plan", &workdir, 5).unwrap();

        let args = fs::read_to_string(args_log).unwrap();
        assert!(!args.contains("--tool-profile"));
    }

    #[test]
    fn times_out_hung_jcode_process() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            "#!/usr/bin/env bash\nset -euo pipefail\nsleep 5\n",
        );
        let runtime = ImplementWorkerRuntime::new(
            "local-coder".to_string(),
            script.display().to_string(),
            "local-coder".to_string(),
            "local-coder".to_string(),
            "http://127.0.0.1:8018/v1".to_string(),
        );

        let error = JCodeProcessRunner::run(&runtime, "fix", &workdir, 1).unwrap_err();

        assert!(error.contains("jcode wall-clock timeout exceeded"));
        assert!(error.contains("local-coder"));
    }

    fn write_script(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-jcode-runner-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
