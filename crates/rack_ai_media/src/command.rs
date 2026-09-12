use std::{
    io::Read,
    process::{Command, Stdio},
};
const MAX_PROBE_BYTES: u64 = 256 * 1024;
pub fn run(program: &str, args: &[&str]) -> Result<String, String> {
    // timeout owns only this bounded inspection/control subprocess, never a GPU process.
    let mut child = Command::new("/usr/bin/timeout")
        .args(["--signal=TERM", "--kill-after=1", "2"])
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("command stdout missing")?
        .take(MAX_PROBE_BYTES + 1)
        .read_to_end(&mut bytes);
    if bytes.len() as u64 > MAX_PROBE_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err("probe output oversized".into());
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    read.map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("bounded {program} failed ({status})"));
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
