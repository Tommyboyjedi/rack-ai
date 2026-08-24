use std::fs;
use std::io;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rack_ai_application::ImplementWorkerRuntime;

use crate::jcode_execution_config::JCodeExecutionConfig;

const PROCESS_GROUP_TERM_GRACE: Duration = Duration::from_millis(500);
const PROCESS_GROUP_KILL_GRACE: Duration = Duration::from_millis(500);
static TEMP_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    let mut command = build_command(runtime, task, workdir, root, &execution_config, network_disabled)
        .map_err(|error| JCodeProcessFailure::new(error, String::new(), String::new()))?;
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
    let status_result = wait_for_completion(&mut child, timeout_seconds, runtime.worker_id());
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

fn build_command(
    runtime: &ImplementWorkerRuntime,
    task: &str,
    workdir: &Path,
    root: &Path,
    execution_config: &JCodeExecutionConfig,
    network_disabled: bool,
) -> Result<Command, String> {
    let mut command = if network_disabled {
        network_isolated_command(runtime)?
    } else {
        Command::new(runtime.entrypoint())
    };
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
    let _ = root;
    let _ = network_disabled;
    Ok(command)
}

fn network_isolated_command(runtime: &ImplementWorkerRuntime) -> Result<Command, String> {
    let uid = current_uid();
    let gid = current_gid();
    let mut command = Command::new("pasta");
    command
        .arg("--foreground")
        .arg("--runas")
        .arg(format!("{uid}:{gid}"))
        .arg("--splice-only")
        .arg(runtime.entrypoint());
    Ok(command)
}

fn current_uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn current_gid() -> u32 {
    unsafe { libc::getegid() }
}

fn wait_for_completion(
    child: &mut Child,
    timeout_seconds: u32,
    worker_id: &str,
) -> Result<ExitStatus, String> {
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
                worker_id,
                timeout_seconds
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn terminate_process_group(child: &mut Child) -> Result<(), String> {
    let pgid = child.id();
    signal_process_group(pgid, libc::SIGTERM)?;
    if wait_for_process_group_exit(child, pgid, PROCESS_GROUP_TERM_GRACE)? {
        return Ok(());
    }
    signal_process_group(pgid, libc::SIGKILL)?;
    if wait_for_process_group_exit(child, pgid, PROCESS_GROUP_KILL_GRACE)? {
        return Ok(());
    }
    Err(format!(
        "process group {} survived timeout cleanup after TERM and KILL",
        pgid
    ))
}

fn wait_for_process_group_exit(child: &mut Child, pgid: u32, grace: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + grace;
    loop {
        reap_if_ready(child)?;
        if !process_group_has_members(pgid)? {
            reap_if_ready(child)?;
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn reap_if_ready(child: &mut Child) -> Result<(), String> {
    if child.try_wait().map_err(|error| error.to_string())?.is_some() {
        return Ok(());
    }
    Ok(())
}

fn process_group_has_members(pgid: u32) -> Result<bool, String> {
    let target = negative_pid(pgid)?;
    let result = unsafe { libc::kill(target, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::ESRCH => Ok(false),
        Some(code) if code == libc::EPERM => Ok(true),
        _ => Err(error.to_string()),
    }
}

fn signal_process_group(pgid: u32, signal: i32) -> Result<(), String> {
    let target = negative_pid(pgid)?;
    let result = unsafe { libc::kill(target, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::ESRCH => Ok(()),
        _ => Err(error.to_string()),
    }
}

fn negative_pid(pgid: u32) -> Result<i32, String> {
    let pgid = i32::try_from(pgid).map_err(|_| format!("invalid process-group id: {}", pgid))?;
    Ok(-pgid)
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
    let counter = TEMP_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("rack-ai-jcode-run-{nanos}-{counter}"));
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
                r#"#!/bin/bash
set -euo pipefail
printf '%s\n' "$@" > '{}'
printf 'trace-line\n' >&2
printf 'COMPLETE\n'
"#,
                args_log.display()
            )
            .as_str(),
        );
        let runtime = coder_runtime(&script);

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
                r#"#!/bin/bash
set -euo pipefail
printf '%s\n' "$@" > '{}'
printf 'COMPLETE\n'
"#,
                args_log.display()
            )
            .as_str(),
        );
        let runtime = primary_runtime(&script);

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
            r#"#!/bin/bash
set -euo pipefail
python3 - <<'PY'
import sys
for _ in range(20000):
    print('stdout-line-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx')
    print('stderr-line-yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy', file=sys.stderr)
PY
"#,
        );
        let runtime = coder_runtime(&script);

        let output = JCodeProcessRunner::run(&runtime, "flood", &workdir, 10, false).unwrap();

        assert!(output.stdout().contains("stdout-line"));
        assert!(output.stderr().contains("stderr-line"));
    }

    #[test]
    fn network_isolation_keeps_loopback_and_blocks_external_even_after_clearing_ld_preload() {
        let _loopback_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let loopback_port = _loopback_listener.local_addr().unwrap().port();
        let _external_listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let external_port = _external_listener.local_addr().unwrap().port();
        let external_host = host_ipv4();
        let shell = format!(
            concat!(
                "set -euo pipefail; ",
                "python3 -c \"import socket; socket.create_connection(('127.0.0.1', {}), timeout=2).close()\"; ",
                "probe=$'import errno, socket\\ntry:\\n    socket.create_connection((\\'{}\\', {}), timeout=2)\\nexcept OSError as error:\\n    raise SystemExit(0 if error.errno == errno.ENETUNREACH else 91)\\nraise SystemExit(92)\\n'; ",
                "python3 -c \"$probe\"; ",
                "env -u LD_PRELOAD python3 -c \"$probe\""
            ),
            loopback_port,
            external_host,
            external_port,
        );
        let output = Command::new("pasta")
            .arg("--foreground")
            .arg("--runas")
            .arg(format!("{}:{}", unsafe { libc::geteuid() }, unsafe { libc::getegid() }))
            .arg("--splice-only")
            .arg("/bin/bash")
            .arg("-lc")
            .arg(&shell)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "expected loopback and env-cleared external probe to succeed inside isolated namespace, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn network_policy_disabled_and_enabled_paths_are_distinguishable() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let _loopback_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let loopback_port = _loopback_listener.local_addr().unwrap().port();
        let _external_listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let external_port = _external_listener.local_addr().unwrap().port();
        let external_host = host_ipv4();
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                r#"#!/bin/bash
set -euo pipefail
python3 - <<'PY'
import socket
socket.create_connection(('127.0.0.1', {loopback_port}), timeout=2).close()
socket.create_connection(('{external_host}', {external_port}), timeout=2).close()
print('COMPLETE')
PY
"#,
                loopback_port = loopback_port,
                external_host = external_host,
                external_port = external_port,
            )
            .as_str(),
        );
        let runtime = coder_runtime(&script);

        let enabled = JCodeProcessRunner::run(&runtime, "network", &workdir, 10, false).unwrap();
        let disabled = JCodeProcessRunner::run(&runtime, "network", &workdir, 10, true).unwrap_err();

        assert!(enabled.stdout().contains("COMPLETE"));
        assert!(disabled.message().contains("jcode exited unsuccessfully"));
    }

    #[test]
    fn times_out_hung_jcode_process_and_kills_descendants_that_ignore_term() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let child_pid = root.join("child.pid");
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                r#"#!/bin/bash
set -euo pipefail
trap 'exit 0' TERM
python3 - <<'PY' &
import os
import signal
import time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
with open('{}', 'w', encoding='utf-8') as handle:
    handle.write(str(os.getpid()))
while True:
    time.sleep(1)
PY
while true; do
  sleep 1
done
"#,
                child_pid.display()
            )
            .as_str(),
        );
        let runtime = coder_runtime(&script);

        let error = JCodeProcessRunner::run(&runtime, "fix", &workdir, 1, false).unwrap_err();
        let pid = fs::read_to_string(&child_pid).unwrap().trim().to_string();
        thread::sleep(Duration::from_millis(200));
        let pid = pid.parse::<i32>().unwrap();
        let alive = unsafe { libc::kill(pid, 0) == 0 };

        assert!(error.message().contains("wall-clock timeout exceeded"));
        assert!(!alive, "TERM-ignoring descendant process should not survive timeout");
    }

    fn coder_runtime(script: &Path) -> ImplementWorkerRuntime {
        ImplementWorkerRuntime::new(
            "local-coder".to_string(),
            script.display().to_string(),
            "local-coder".to_string(),
            "local-coder".to_string(),
            "http://127.0.0.1:8018/v1".to_string(),
        )
        .with_tool_profile(Some("minimal".to_string()))
        .with_context_window(Some(16368))
    }

    fn primary_runtime(script: &Path) -> ImplementWorkerRuntime {
        ImplementWorkerRuntime::new(
            "local-primary".to_string(),
            script.display().to_string(),
            "local-primary".to_string(),
            "local-primary".to_string(),
            "http://127.0.0.1:8017/v1".to_string(),
        )
    }

    fn host_ipv4() -> String {
        let output = Command::new("/usr/sbin/ip")
            .args(["-o", "-4", "addr", "show", "scope", "global"])
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let line = stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .expect("expected a non-loopback IPv4 address for network isolation tests");
        let address = line
            .split_whitespace()
            .find(|part| part.contains('/'))
            .expect("expected CIDR token in ip output");
        address.split('/').next().unwrap().to_string()
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
        let counter = super::TEMP_ROOT_COUNTER.fetch_add(1, super::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rack-ai-jcode-runner-{nanos}-{counter}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
