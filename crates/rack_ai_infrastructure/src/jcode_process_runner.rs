use std::fs;
use std::io;
use std::io::Read;
use std::net::Shutdown;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

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
        let result = run_with_root(
            runtime,
            task,
            workdir,
            timeout_seconds,
            network_disabled,
            &root,
        );
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
    let mut prepared = build_command(
        runtime,
        task,
        workdir,
        root,
        &execution_config,
        network_disabled,
    )
    .map_err(|error| JCodeProcessFailure::new(error, String::new(), String::new()))?;
    prepared.command.process_group(0);
    let mut child = prepared.command.spawn().map_err(|error| {
        JCodeProcessFailure::new(error.to_string(), String::new(), String::new())
    })?;
    let stdout_handle = spawn_reader(child.stdout.take().ok_or_else(|| {
        JCodeProcessFailure::new(
            "missing JCode stdout pipe".to_string(),
            String::new(),
            String::new(),
        )
    })?);
    let stderr_handle = spawn_reader(child.stderr.take().ok_or_else(|| {
        JCodeProcessFailure::new(
            "missing JCode stderr pipe".to_string(),
            String::new(),
            String::new(),
        )
    })?);
    let status_result = wait_for_completion(
        &mut child,
        timeout_seconds,
        runtime.worker_id(),
        network_disabled,
    );
    let stdout = collect_reader(stdout_handle);
    let stderr = collect_reader(stderr_handle);
    let _ = prepared.isolation.take();
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

struct PreparedCommand {
    command: Command,
    isolation: Option<NetworkIsolationGuard>,
}

struct NetworkIsolationGuard {
    host_bridge: HostUnixBridge,
}

fn build_command(
    runtime: &ImplementWorkerRuntime,
    task: &str,
    workdir: &Path,
    root: &Path,
    execution_config: &JCodeExecutionConfig,
    network_disabled: bool,
) -> Result<PreparedCommand, String> {
    let mut prepared = if network_disabled {
        prepare_bubblewrap_command(runtime, workdir, root)?
    } else {
        PreparedCommand {
            command: Command::new(runtime.entrypoint()),
            isolation: None,
        }
    };
    prepared
        .command
        .arg("--no-update")
        .arg("--no-selfdev")
        .arg("--quiet")
        .arg("--trace")
        .arg("--provider-profile")
        .arg(runtime.provider_profile())
        .arg("--model")
        .arg(runtime.api_model_id());
    if let Some(tool_profile) = runtime.tool_profile() {
        prepared.command.arg("--tool-profile").arg(tool_profile);
    }
    prepared
        .command
        .arg("-C")
        .arg(workdir)
        .arg("run")
        .arg(task)
        .current_dir(workdir)
        .env("HOME", execution_config.home_dir())
        .env(
            "XDG_CONFIG_HOME",
            execution_config.home_dir().join(".config"),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(prepared)
}

fn prepare_bubblewrap_command(
    runtime: &ImplementWorkerRuntime,
    workdir: &Path,
    root: &Path,
) -> Result<PreparedCommand, String> {
    let endpoint = LocalEndpoint::parse(runtime.endpoint())?;
    let socket_path = root.join("selected-vllm.sock");
    let launcher_path = root.join("sandbox-launcher.sh");
    let host_bridge = HostUnixBridge::start(&socket_path, endpoint.port)?;
    let bridge_command = bridge_command(root, &socket_path, endpoint.port)?;
    write_launcher_script(&launcher_path, &bridge_command)?;

    let mut command = Command::new("bwrap");
    command
        .arg("--unshare-net")
        .arg("--unshare-pid")
        .arg("--die-with-parent")
        .arg("--ro-bind")
        .arg("/")
        .arg("/")
        .arg("--bind")
        .arg(workdir)
        .arg(workdir)
        .arg("--bind")
        .arg(root)
        .arg(root)
        .arg("--dev")
        .arg("/dev")
        .arg("--proc")
        .arg("/proc")
        .arg("--chdir")
        .arg(workdir)
        .arg("/bin/bash")
        .arg("--noprofile")
        .arg("--norc")
        .arg(launcher_path)
        .arg(runtime.entrypoint());

    Ok(PreparedCommand {
        command,
        isolation: Some(NetworkIsolationGuard { host_bridge }),
    })
}

#[derive(Clone, Copy)]
struct LocalEndpoint {
    port: u16,
}

impl LocalEndpoint {
    fn parse(endpoint: &str) -> Result<Self, String> {
        let remainder = endpoint
            .strip_prefix("http://127.0.0.1:")
            .ok_or_else(|| format!("expected local loopback endpoint, found {}", endpoint))?;
        let port_text = remainder
            .split('/')
            .next()
            .ok_or_else(|| format!("missing endpoint port in {}", endpoint))?;
        let port = port_text
            .parse::<u16>()
            .map_err(|error| format!("invalid endpoint port in {}: {}", endpoint, error))?;
        Ok(Self { port })
    }
}

struct HostUnixBridge {
    socket_path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl HostUnixBridge {
    fn start(socket_path: &Path, target_port: u16) -> Result<Self, String> {
        let _ = fs::remove_file(socket_path);
        let listener = UnixListener::bind(socket_path).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let socket_path_buf = socket_path.to_path_buf();
        let thread = thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        thread::spawn(move || {
                            let _ = bridge_unix_to_tcp(stream, target_port);
                        });
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
            let _ = fs::remove_file(&socket_path_buf);
        });
        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for HostUnixBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = UnixStream::connect(&self.socket_path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

impl Drop for NetworkIsolationGuard {
    fn drop(&mut self) {
        let _ = &self.host_bridge;
    }
}

fn bridge_unix_to_tcp(stream: UnixStream, target_port: u16) -> Result<(), String> {
    let upstream =
        TcpStream::connect(("127.0.0.1", target_port)).map_err(|error| error.to_string())?;
    let mut stream_read = stream.try_clone().map_err(|error| error.to_string())?;
    let mut stream_write = stream;
    let mut upstream_read = upstream.try_clone().map_err(|error| error.to_string())?;
    let mut upstream_write = upstream;
    let left = thread::spawn(move || {
        let _ = io::copy(&mut stream_read, &mut upstream_write);
        let _ = upstream_write.shutdown(Shutdown::Write);
    });
    let _ = io::copy(&mut upstream_read, &mut stream_write);
    let _ = stream_write.shutdown(Shutdown::Write);
    let _ = left.join();
    Ok(())
}

#[cfg(test)]
fn bridge_command(root: &Path, socket_path: &Path, listen_port: u16) -> Result<String, String> {
    let local_bridge_path = root.join("sandbox-local-bridge.py");
    write_test_local_bridge_script(&local_bridge_path)?;
    Ok(format!(
        "python3 {} {} {}",
        shell_quote_path(&local_bridge_path),
        shell_quote_path(socket_path),
        listen_port
    ))
}

#[cfg(not(test))]
fn bridge_command(_root: &Path, socket_path: &Path, listen_port: u16) -> Result<String, String> {
    let current_executable = std::env::current_exe().map_err(|error| error.to_string())?;
    Ok(format!(
        "{} __sandbox-tcp-bridge {} {}",
        shell_quote_path(&current_executable),
        shell_quote_path(socket_path),
        listen_port
    ))
}

#[cfg(test)]
fn write_test_local_bridge_script(path: &Path) -> Result<(), String> {
    write_executable(
        path,
        r#"import socket
import sys
import threading

unix_path = sys.argv[1]
listen_port = int(sys.argv[2])
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", listen_port))
server.listen(16)

def pipe(src, dst):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    finally:
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass

def handle(client):
    upstream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    upstream.connect(unix_path)
    threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
    pipe(upstream, client)
    client.close()
    upstream.close()

while True:
    client, _ = server.accept()
    threading.Thread(target=handle, args=(client,), daemon=True).start()
"#,
    )
}

fn write_launcher_script(path: &Path, bridge_command: &str) -> Result<(), String> {
    write_executable(
        path,
        &format!(
            concat!(
                "#!/bin/bash\n",
                "set -euo pipefail\n",
                "{} >/dev/null 2>&1 &\n",
                "bridge_pid=$!\n",
                "cleanup() {{\n",
                "  kill \"$bridge_pid\" 2>/dev/null || true\n",
                "  wait \"$bridge_pid\" 2>/dev/null || true\n",
                "}}\n",
                "trap cleanup EXIT\n",
                "\"$@\" &\n",
                "child_pid=$!\n",
                "wait \"$child_pid\"\n"
            ),
            bridge_command,
        ),
    )
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(path.to_string_lossy().as_ref())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn write_executable(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|error| error.to_string())?;
    let mut permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

fn wait_for_completion(
    child: &mut Child,
    timeout_seconds: u32,
    worker_id: &str,
    network_disabled: bool,
) -> Result<ExitStatus, String> {
    let timeout_seconds = timeout_seconds.max(1);
    let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_seconds));
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            terminate_execution(child, network_disabled)?;
            return Err(format!(
                "jcode wall-clock timeout exceeded for worker {} after {} seconds",
                worker_id, timeout_seconds
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn terminate_execution(child: &mut Child, network_disabled: bool) -> Result<(), String> {
    let pgid = child.id();
    signal_process_group(pgid, libc::SIGTERM)?;
    if wait_for_process_group_exit(child, pgid, PROCESS_GROUP_TERM_GRACE)? {
        return Ok(());
    }
    if network_disabled {
        signal_pid(child.id(), libc::SIGKILL)?;
    }
    signal_process_group(pgid, libc::SIGKILL)?;
    if wait_for_child_exit(child, PROCESS_GROUP_KILL_GRACE)?
        && wait_for_process_group_exit(child, pgid, Duration::from_millis(50))?
    {
        return Ok(());
    }
    Err(format!(
        "process tree for pid {} survived timeout cleanup",
        child.id()
    ))
}

fn wait_for_child_exit(child: &mut Child, grace: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + grace;
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_process_group_exit(
    child: &mut Child,
    pgid: u32,
    grace: Duration,
) -> Result<bool, String> {
    let deadline = Instant::now() + grace;
    loop {
        let _ = child.try_wait().map_err(|error| error.to_string())?;
        if !process_group_has_members(pgid)? {
            let _ = child.try_wait().map_err(|error| error.to_string())?;
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(50));
    }
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
    signal_pid_value(target, signal)
}

fn signal_pid(pid: u32, signal: i32) -> Result<(), String> {
    let pid = i32::try_from(pid).map_err(|_| format!("invalid pid: {}", pid))?;
    signal_pid_value(pid, signal)
}

fn signal_pid_value(pid: i32, signal: i32) -> Result<(), String> {
    let result = unsafe { libc::kill(pid, signal) };
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
    use std::io::Write;
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;

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
        let runtime = coder_runtime(&script, "http://127.0.0.1:8018/v1");

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
        let runtime = primary_runtime(&script, "http://127.0.0.1:8017/v1");

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
        let runtime = coder_runtime(&script, "http://127.0.0.1:8018/v1");

        let output = JCodeProcessRunner::run(&runtime, "flood", &workdir, 10, false).unwrap();

        assert!(output.stdout().contains("stdout-line"));
        assert!(output.stderr().contains("stderr-line"));
    }

    #[test]
    fn network_isolation_keeps_selected_loopback_and_blocks_external_even_after_clearing_ld_preload()
     {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let selected = TcpListener::bind("127.0.0.1:0").unwrap();
        let selected_port = selected.local_addr().unwrap().port();
        let selected_server = thread::spawn(move || {
            let (mut stream, _) = selected.accept().unwrap();
            let _ = stream.write_all(b"ok");
        });
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                r#"#!/bin/bash
set -euo pipefail
python3 - <<'PY'
import subprocess
import socket
socket.create_connection(('127.0.0.1', {selected_port}), timeout=2).close()
probe = """import errno, socket
try:
    socket.create_connection(('8.8.8.8', 53), timeout=2)
except OSError as error:
    raise SystemExit(0 if error.errno == errno.ENETUNREACH else 91)
raise SystemExit(92)
"""
subprocess.run(['python3', '-c', probe], check=True)
subprocess.run(['env', '-u', 'LD_PRELOAD', 'python3', '-c', probe], check=True)
print('COMPLETE')
PY
"#,
                selected_port = selected_port,
            )
            .as_str(),
        );
        let runtime = coder_runtime(&script, &format!("http://127.0.0.1:{selected_port}/v1"));

        let output = JCodeProcessRunner::run(&runtime, "network", &workdir, 10, true).unwrap();
        selected_server.join().unwrap();

        assert!(output.stdout().contains("COMPLETE"));
    }

    #[test]
    fn network_policy_disabled_and_enabled_paths_are_distinguishable() {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let selected = TcpListener::bind("127.0.0.1:0").unwrap();
        let selected_port = selected.local_addr().unwrap().port();
        let external = TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let external_port = external.local_addr().unwrap().port();
        let external_host = host_ipv4();
        let selected_server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = selected.accept().unwrap();
                let _ = stream.write_all(b"ok");
            }
        });
        let external_server = thread::spawn(move || {
            let (mut stream, _) = external.accept().unwrap();
            let _ = stream.write_all(b"ok");
        });
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                r#"#!/bin/bash
set -euo pipefail
python3 - <<'PY'
import socket
socket.create_connection(('127.0.0.1', {selected_port}), timeout=2).close()
socket.create_connection(('{external_host}', {external_port}), timeout=2).close()
print('COMPLETE')
PY
"#,
                selected_port = selected_port,
                external_host = external_host,
                external_port = external_port,
            )
            .as_str(),
        );
        let runtime = coder_runtime(&script, &format!("http://127.0.0.1:{selected_port}/v1"));

        let enabled = JCodeProcessRunner::run(&runtime, "network", &workdir, 10, false).unwrap();
        let disabled =
            JCodeProcessRunner::run(&runtime, "network", &workdir, 10, true).unwrap_err();
        selected_server.join().unwrap();
        external_server.join().unwrap();

        assert!(enabled.stdout().contains("COMPLETE"));
        assert!(disabled.message().contains("jcode exited unsuccessfully"));
    }

    #[test]
    fn times_out_hung_jcode_process_and_kills_descendants_that_ignore_term_and_escape_process_groups()
     {
        let root = temp_root();
        let workdir = root.join("worktree");
        fs::create_dir_all(&workdir).unwrap();
        let marker = format!("rack-ai-bwrap-timeout-{}", root.display());
        let script = root.join("fake-jcode.sh");
        write_script(
            &script,
            format!(
                r#"#!/bin/bash
set -euo pipefail
trap 'exit 0' TERM
python3 - <<'PY'
import signal
import subprocess
import time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
subprocess.Popen(['python3', '-c', 'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)', '{marker}-ignore'])
subprocess.Popen(['python3', '-c', 'import os,subprocess,time; os.setsid(); subprocess.Popen(["python3","-c","import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)","{marker}-grand"]); time.sleep(300)', '{marker}-setsid'])
time.sleep(300)
PY
"#,
                marker = marker,
            )
            .as_str(),
        );
        let runtime = coder_runtime(&script, "http://127.0.0.1:8018/v1");

        let error = JCodeProcessRunner::run(&runtime, "fix", &workdir, 1, true).unwrap_err();
        thread::sleep(Duration::from_millis(200));
        let survivors = Command::new("/bin/bash")
            .arg("-lc")
            .arg(format!("ps -ef | grep '{}' | grep -v grep", marker))
            .output()
            .unwrap();

        assert!(error.message().contains("wall-clock timeout exceeded"));
        assert!(
            survivors.stdout.is_empty(),
            "all descendants must be gone after timeout: {}",
            String::from_utf8_lossy(&survivors.stdout)
        );
    }

    fn coder_runtime(script: &Path, endpoint: &str) -> ImplementWorkerRuntime {
        ImplementWorkerRuntime::new(
            "local-coder".to_string(),
            script.display().to_string(),
            "local-coder".to_string(),
            "local-coder".to_string(),
            endpoint.to_string(),
        )
        .with_tool_profile(Some("minimal".to_string()))
        .with_context_window(Some(16368))
    }

    fn primary_runtime(script: &Path, endpoint: &str) -> ImplementWorkerRuntime {
        ImplementWorkerRuntime::new(
            "local-primary".to_string(),
            script.display().to_string(),
            "local-primary".to_string(),
            "local-primary".to_string(),
            endpoint.to_string(),
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
