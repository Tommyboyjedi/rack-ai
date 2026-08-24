use std::fs;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rack_ai_application::ImplementWorkerRuntime;

use crate::jcode_execution_config::JCodeExecutionConfig;
use crate::jcode_network_guard;

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

#[derive(Debug)]
pub struct JCodeProcessFailure {
    message: String,
    stdout: String,
    stderr: String,
}

impl JCodeProcessFailure {
    pub fn new(message: String, stdout: String, stderr: String) -> Self {
        Self {
            message,
            stdout,
            stderr,
        }
    }

    pub fn message(&self) -> &str {
        self.message.as_str()
    }

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
        network_disabled: bool,
    ) -> Result<JCodeProcessOutput, JCodeProcessFailure> {
        let root = temp_root();
        let result = run_with_root(runtime, task, workdir, timeout_seconds, network_disabled, &root);
        let _ = fs::remove_dir_all(&root);
        result
    }
}

fn run_with_root(
    runtime: &ImplementWorkerRuntime,
    task: &str,
    workdir: &Path,
    timeout_seconds: u32,
    network_disabled: bool,
    root: &Path,
) -> Result<JCodeProcessOutput, JCodeProcessFailure> {
    let execution_config = JCodeExecutionConfig::prepare_at(root, runtime)
        .map_err(|error| JCodeProcessFailure::new(error, String::new(), String::new()))?;
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
        .current_dir(workdir)
        .env("HOME", execution_config.home_dir())
        .env("XDG_CONFIG_HOME", execution_config.home_dir().join(".config"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if network_disabled {
        let guard = jcode_network_guard::compile_at(root)
            .map_err(|error| JCodeProcessFailure::new(error, String::new(), String::new()))?;
        command.env("LD_PRELOAD", &guard);
    }
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| JCodeProcessFailure::new(error.to_string(), String::new(), String::new()))?;
    let stdout_handle = spawn_reader(
        child
            .stdout
            .take()
            .ok_or_else(|| JCodeProcessFailure::new("missing JCode stdout pipe".to_string(), String::new(), String::new()))?,
    );
    let stderr_handle = spawn_reader(
        child
            .stderr
            .take()
            .ok_or_else(|| JCodeProcessFailure::new("missing JCode stderr pipe".to_string(), String::new(), String::new()))?,
    );
    let status_result = wait_for_completion(&mut child, timeout_seconds);
    let stdout = collect_reader(stdout_handle);
    let stderr = collect_reader(stderr_handle);
    let status = status_result
        .map_err(|error| JCodeProcessFailure::new(error, stdout.clone(), stderr.clone()))?;
    if !status.success() {
        return Err(JCodeProcessFailure::new(
            format!(
                "jcode exited unsuccessfully for worker {}: {}",
                runtime.worker_id(),
                stderr.trim()
            ),
            stdout,
            stderr,
        ));
    }
    Ok(JCodeProcessOutput { stdout, stderr })
}

fn wait_for_completion(child: &mut Child, timeout_seconds: u32) -> Result<ExitStatus, String> {
    let timeout_seconds = timeout_seconds.max(1);
    let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_seconds));
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            terminate_process_group(child)?;
            return Err(format!(
                "jcode wall-clock timeout exceeded for worker {} after {} seconds",
                child.id(),
                timeout_seconds
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn terminate_process_group(child: &mut Child) -> Result<(), String> {
    signal_process_group(child.id(), "TERM")?;
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        if child.try_wait().map_err(|error| error.to_string())?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    signal_process_group(child.id(), "KILL")?;
    let _ = child.wait();
    Ok(())
}

fn signal_process_group(pid: u32, signal: &str) -> Result<(), String> {
    let status = Command::new("/bin/kill")
        .arg(format!("-{signal}"))
        .arg(format!("-{pid}"))
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        return Ok(());
    }
    Ok(())
}

fn spawn_reader<T: Read + Send + 'static>(mut reader: T) -> JoinHandle<Result<String, String>> {
    thread::spawn(move || {
        let mut text = String::new();
        reader
            .read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        Ok(text)
    })
}

fn collect_reader(handle: JoinHandle<Result<String, String>>) -> String {
    match handle.join() {
        Ok(Ok(text)) => text,
        Ok(Err(error)) => format!("<<reader error: {error}>>"),
        Err(_) => "<<reader panicked>>".to_string(),
    }
}

fn temp_root() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rack-ai-jcode-run-{nanos}"));
    fs::create_dir_all(&root).unwrap();
    root
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        .with_tool_profile(Some("minimal".to_string()))
        .with_context_window(Some(16368));

        let output = JCodeProcessRunner::run(&runtime, "fix the file", &workdir, 5, false).unwrap();

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

        JCodeProcessRunner::run(&runtime, "plan", &workdir, 5, false).unwrap();

        let args = fs::read_to_string(args_log).unwrap();
        assert!(!args.contains("--tool-profile"));
    }

    #[test]
    fn drains_large_stdout_and_stderr_without_deadlock() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            "#!/usr/bin/env bash\nset -euo pipefail\npython3 - <<'PY'\nimport sys\nfor _ in range(20000):\n    print('stdout-line-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx')\n    print('stderr-line-yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy', file=sys.stderr)\nPY\n",
        );
        let runtime = ImplementWorkerRuntime::new(
            "local-coder".to_string(),
            script.display().to_string(),
            "local-coder".to_string(),
            "local-coder".to_string(),
            "http://127.0.0.1:8018/v1".to_string(),
        )
        .with_tool_profile(Some("minimal".to_string()))
        .with_context_window(Some(16368));

        let output = JCodeProcessRunner::run(&runtime, "flood", &workdir, 10, false).unwrap();

        assert!(output.stdout().contains("stdout-line"));
        assert!(output.stderr().contains("stderr-line"));
    }

    #[test]
    fn enforces_loopback_only_network_policy_for_jcode_and_descendants() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = std::io::Write::write_all(&mut stream, b"ok");
        });
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                concat!(
                    "#!/usr/bin/env bash\n",
                    "set -euo pipefail\n",
                    "python3 - <<'PY'\n",
                    "import errno\n",
                    "import os\n",
                    "import socket\n",
                    "sock = socket.create_connection(('127.0.0.1', {port}), timeout=2)\n",
                    "sock.recv(2)\n",
                    "sock.close()\n",
                    "try:\n",
                    "    socket.create_connection(('8.8.8.8', 53), timeout=2)\n",
                    "except OSError as error:\n",
                    "    if error.errno != errno.ENETUNREACH:\n",
                    "        raise SystemExit(f'unexpected errno: {{error.errno}}')\n",
                    "else:\n",
                    "    raise SystemExit('external network unexpectedly succeeded')\n",
                    "print('COMPLETE')\n",
                    "PY\n"
                ),
                port = port,
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
        .with_tool_profile(Some("minimal".to_string()))
        .with_context_window(Some(16368));

        let output = JCodeProcessRunner::run(&runtime, "network", &workdir, 10, true).unwrap();
        server.join().unwrap();

        assert!(output.stdout().contains("COMPLETE"));
    }

    #[test]
    fn times_out_hung_jcode_process_and_kills_descendants() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let child_pid = root.join("child.pid");
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                concat!(
                    "#!/usr/bin/env bash\n",
                    "set -euo pipefail\n",
                    "sleep 30 &\n",
                    "echo $! > '{}'\n",
                    "wait\n"
                ),
                child_pid.display()
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
        .with_tool_profile(Some("minimal".to_string()))
        .with_context_window(Some(16368));

        let error = JCodeProcessRunner::run(&runtime, "fix", &workdir, 1, false).unwrap_err();
        let pid = fs::read_to_string(&child_pid).unwrap().trim().to_string();
        thread::sleep(Duration::from_millis(200));
        let alive = Command::new("/bin/kill")
            .arg("-0")
            .arg(&pid)
            .status()
            .unwrap()
            .success();

        assert!(error.message().contains("wall-clock timeout exceeded"));
        assert!(!alive, "descendant process should not survive timeout");
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
