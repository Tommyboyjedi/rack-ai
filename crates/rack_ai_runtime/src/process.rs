use crate::{config::Profile, types::Process};
use std::{fs, path::PathBuf};
pub fn capture(pid: u32, activation: &str) -> Result<Process, String> {
    if pid == 0 {
        return Err("missing_process".into());
    }
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|e| e.to_string())?;
    let fields: Vec<_> = stat
        .rsplit_once(") ")
        .ok_or("invalid_proc_stat")?
        .1
        .split_whitespace()
        .collect();
    if fields.first() == Some(&"Z") {
        return Err("process_is_zombie".into());
    }
    Ok(Process {
        pid,
        boot: fs::read_to_string("/proc/sys/kernel/random/boot_id").map_err(|e| e.to_string())?,
        start: fields.get(19).ok_or("missing_start_ticks")?.to_string(),
        activation: activation.into(),
        unit: None,
        container: None,
        invocation: None,
    })
}
pub fn verify(process: &Process, profile: &Profile) -> Result<(), String> {
    let current = capture(process.pid, &process.activation)?;
    if current.boot != process.boot || current.start != process.start {
        return Err("process_generation_changed".into());
    }
    if process.container.is_some() {
        return crate::container::verify(process, profile);
    }
    let root = PathBuf::from(format!("/proc/{}", process.pid));
    if fs::read_link(root.join("exe")).map_err(|e| e.to_string())?
        != profile
            .executable
            .canonicalize()
            .map_err(|e| e.to_string())?
    {
        return Err("process_executable_mismatch".into());
    }
    let env = fs::read(root.join("environ")).map_err(|e| e.to_string())?;
    let expected = format!("RACK_RUNTIME_ACTIVATION={}", process.activation);
    if !env.split(|b| *b == 0).any(|e| e == expected.as_bytes()) {
        return Err("activation_mismatch".into());
    }
    Ok(())
}
pub fn gone(process: &Process) -> Result<bool, String> {
    if fs::read_to_string("/proc/sys/kernel/random/boot_id").map_err(|e| e.to_string())?
        != process.boot
    {
        return Ok(true);
    }
    if !PathBuf::from(format!("/proc/{}", process.pid))
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(true);
    }
    match capture(process.pid, &process.activation) {
        Ok(p) => Ok(p.start != process.start),
        Err(e) if e == "process_is_zombie" => Ok(true),
        Err(e) => Err(e),
    }
}
pub fn pids(process: &Process) -> Result<Vec<u32>, String> {
    if let Some(unit) = &process.unit {
        let observed = rack_ai_media::systemd::Systemd { unit: unit.clone() }.observe()?;
        if process.invocation.as_ref() != Some(&observed.invocation) {
            return Err("systemd_invocation_changed".into());
        }
        let file = PathBuf::from("/sys/fs/cgroup")
            .join(observed.cgroup.trim_start_matches('/'))
            .join("cgroup.procs");
        return fs::read_to_string(file)
            .map_err(|e| e.to_string())?
            .lines()
            .map(|l| l.parse().map_err(|_| "invalid_cgroup_pid".into()))
            .collect();
    }
    if process.container.is_some() {
        let raw = rack_ai_media::command::run(
            "docker",
            &[
                "top",
                process.container.as_ref().ok_or("missing_container")?,
                "-eo",
                "pid",
            ],
        )?;
        return raw
            .lines()
            .skip(1)
            .map(|s| s.trim().parse().map_err(|_| "invalid_container_pid".into()))
            .collect();
    }
    Ok(vec![process.pid])
}
pub fn endpoint_owned(process: &Process, profile: &Profile) -> Result<(), String> {
    verify(process, profile)?;
    let url = reqwest::Url::parse(&profile.endpoint).map_err(|e| e.to_string())?;
    let port = format!("{:04X}", url.port().ok_or("missing_port")?);
    let mut inodes = Vec::new();
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        for line in fs::read_to_string(path)
            .map_err(|e| e.to_string())?
            .lines()
            .skip(1)
        {
            let f: Vec<_> = line.split_whitespace().collect();
            if f.len() > 9 && f[3] == "0A" && f[1].ends_with(&format!(":{port}")) {
                inodes.push(format!("socket:[{}]", f[9]));
            }
        }
    }
    if inodes.len() != 1 {
        return Err("endpoint_listener_ambiguous".into());
    }
    if let Some(container) = &process.container {
        let descriptors = rack_ai_media::command::run(
            "docker",
            &[
                "exec",
                container,
                "/bin/sh",
                "-c",
                "find /proc/[0-9]*/fd -maxdepth 1 -type l -exec readlink {} \\; 2>/dev/null",
            ],
        )?;
        return if descriptors.lines().any(|line| line == inodes[0]) {
            Ok(())
        } else {
            Err("endpoint_not_owned_by_container".into())
        };
    }
    let mut owners = vec![process.pid];
    owners.extend(pids(process)?.into_iter().filter(|pid| *pid != process.pid));
    for pid in owners {
        for entry in fs::read_dir(format!("/proc/{pid}/fd")).map_err(|e| e.to_string())? {
            let link = fs::read_link(entry.map_err(|e| e.to_string())?.path());
            if link.is_ok_and(|p| p.to_string_lossy() == inodes[0]) {
                return Ok(());
            }
        }
    }
    Err("endpoint_not_owned_by_activation".into())
}
