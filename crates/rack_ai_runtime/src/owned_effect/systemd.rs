use crate::{process, types::*};
use rack_ai_media::{command::run, systemd::Systemd};
#[path = "systemd_probe.rs"]
mod probe;
pub(super) use probe::validate_metadata;
use probe::{cgroup_empty, loaded, require_saved_gone, wait_absent};

pub(super) fn resolve(d: &Demand) -> Result<Option<Process>, String> {
    let unit = format!("rack-runtime-{}.service", d.generation);
    if let Some(saved) = &d.process
        && (saved.unit.as_ref() != Some(&unit) || saved.container.is_some())
    {
        return Err("recovery_systemd_identity_changed".into());
    }
    if !loaded(&unit)? {
        require_saved_gone(d)?;
        return Ok(None);
    }
    let systemd = Systemd { unit: unit.clone() };
    let observed = systemd.observe()?;
    let raw = run(
        "systemctl",
        &[
            "--user",
            "show",
            &unit,
            "-p",
            "Transient",
            "-p",
            "Restart",
            "-p",
            "Environment",
        ],
    )?;
    validate_metadata(&raw, &d.generation)?;
    if let Some(saved) = &d.process
        && !observed.invocation.is_empty()
        && saved.invocation.as_ref() != Some(&observed.invocation)
    {
        return Err("recovery_systemd_invocation_changed".into());
    }
    if observed.pending_job && observed.pid != 0 {
        return Err("recovery_systemd_job_ambiguous".into());
    }
    if observed.pid == 0 {
        require_saved_gone(d)?;
        if observed.pending_job
            || !matches!(observed.active.as_str(), "inactive" | "failed")
            || !cgroup_empty(&observed.cgroup)?
        {
            if let Some(saved) = &d.process
                && !observed.pending_job
                && !observed.invocation.is_empty()
            {
                return Ok(Some(saved.clone()));
            }
            // Exact transient unit + activation environment proves this pending
            // start or descendant-only effect is ours even without a captured PID.
            run("systemctl", &["--user", "stop", "--no-block", &unit])?;
            wait_absent(d, &observed.invocation)?;
        }
        return Ok(None);
    }
    if observed.invocation.is_empty() {
        return Err("recovery_systemd_invocation_missing".into());
    }
    let mut p = process::capture(observed.pid, &d.generation)?;
    p.unit = Some(unit);
    p.invocation = Some(observed.invocation.clone());
    if let Some(saved) = &d.process
        && *saved != p
    {
        return Err("recovery_systemd_process_changed".into());
    }
    process::verify(&p, &d.profile)?;
    let after = systemd.observe()?;
    if after.pid != p.pid || after.invocation != observed.invocation || after.pending_job {
        return Err("recovery_systemd_observation_changed".into());
    }
    Ok(Some(p))
}
