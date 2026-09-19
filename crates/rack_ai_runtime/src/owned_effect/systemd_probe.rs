use crate::{process, types::*};
use rack_ai_media::{command::run, systemd::Systemd};
use std::{collections::BTreeMap, path::Path};

pub(super) fn wait_absent(d: &Demand, invocation: &str) -> Result<(), String> {
    let unit = format!("rack-runtime-{}.service", d.backend_activation());
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(d.profile.stop_seconds);
    loop {
        if !loaded(&unit)? {
            return Ok(());
        }
        let observed = Systemd { unit: unit.clone() }.observe()?;
        if !observed.invocation.is_empty() && observed.invocation != invocation {
            return Err("recovery_systemd_invocation_changed".into());
        }
        if observed.pid != 0 {
            return Err("recovery_systemd_process_changed".into());
        }
        if !observed.pending_job
            && matches!(observed.active.as_str(), "inactive" | "failed")
            && cgroup_empty(&observed.cgroup)?
        {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err("recovery_systemd_cleanup_unproven".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

pub(super) fn require_saved_gone(d: &Demand) -> Result<(), String> {
    if let Some(saved) = &d.process
        && !process::gone(saved)?
    {
        return Err("recovery_systemd_process_still_live".into());
    }
    Ok(())
}

pub(super) fn loaded(unit: &str) -> Result<bool, String> {
    let units = run(
        "systemctl",
        &[
            "--user",
            "list-units",
            "--all",
            "--plain",
            "--no-legend",
            "--full",
            unit,
        ],
    )?;
    if !units.trim().is_empty() {
        if units.lines().count() != 1 || units.split_whitespace().next() != Some(unit) {
            return Err("recovery_systemd_unit_ambiguous".into());
        }
        return Ok(true);
    }
    let files = run(
        "systemctl",
        &["--user", "list-unit-files", "--no-legend", unit],
    )?;
    if !files.trim().is_empty() {
        return Err("recovery_systemd_foreign_unit_file".into());
    }
    Ok(false)
}

pub(super) fn cgroup_empty(cgroup: &str) -> Result<bool, String> {
    if cgroup.is_empty() {
        return Ok(true);
    }
    let path = Path::new("/sys/fs/cgroup").join(cgroup.trim_start_matches('/'));
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(true);
    }
    let processes =
        std::fs::read_to_string(path.join("cgroup.procs")).map_err(|e| e.to_string())?;
    let events = std::fs::read_to_string(path.join("cgroup.events")).map_err(|e| e.to_string())?;
    Ok(processes.trim().is_empty() && events.lines().any(|line| line == "populated 0"))
}

pub(crate) fn validate_metadata(raw: &str, generation: &str) -> Result<(), String> {
    let values: BTreeMap<_, _> = raw
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let expected = format!("RACK_RUNTIME_ACTIVATION={generation}");
    let env: Vec<_> = values
        .get("Environment")
        .unwrap_or(&"")
        .split_whitespace()
        .filter(|entry| entry.starts_with("RACK_RUNTIME_ACTIVATION="))
        .collect();
    if values.get("Transient") != Some(&"yes")
        || values.get("Restart") != Some(&"no")
        || env != vec![expected.as_str()]
    {
        return Err("recovery_systemd_ownership_unproven".into());
    }
    Ok(())
}
