//! Bounded proof of process and transient-unit teardown, never a stop retry.
use crate::{process, types::*};
use std::{
    path::Path,
    time::{Duration, Instant},
};
pub struct Teardown;
impl Teardown {
    pub fn wait(d: &Demand) -> Result<(), String> {
        let p = d.process.as_ref().ok_or("missing_cleanup_process")?;
        let deadline = Instant::now() + Duration::from_secs(d.profile.stop_seconds);
        loop {
            let gone = process::gone(p)?;
            if gone
                && let Ok(current) = process::capture(p.pid, &p.activation)
                && current.boot == p.boot
                && current.start != p.start
            {
                return Err("process_generation_changed".into());
            }
            let unit_gone = Self::unit_gone(p)?;
            if gone && unit_gone {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("stop_deadline_cleanup_unproven".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn unit_gone(p: &Process) -> Result<bool, String> {
        let Some(unit) = &p.unit else {
            return Ok(true);
        };
        if unit != &format!("rack-runtime-{}.service", p.activation) {
            return Err("systemd_activation_changed".into());
        }
        let expected = p
            .invocation
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or("systemd_invocation_unknown")?;
        let observed = rack_ai_media::systemd::Systemd { unit: unit.clone() }.observe()?;
        if !observed.invocation.is_empty() && observed.invocation != expected {
            return Err("systemd_invocation_changed".into());
        }
        if observed.pid != 0 && observed.pid != p.pid {
            return Err("systemd_process_changed".into());
        }
        let inactive = matches!(observed.active.as_str(), "inactive" | "failed");
        if observed.invocation.is_empty() && (observed.pid != 0 || !inactive) {
            return Err("systemd_ownership_uncertain".into());
        }
        if observed.pending_job || observed.pid != 0 || !inactive {
            return Ok(false);
        }
        if observed.cgroup.is_empty() {
            return Ok(true);
        }
        // For these transient units, even an empty lingering cgroup must finish
        // disappearing. An inaccessible path is uncertainty, not absence.
        Path::new("/sys/fs/cgroup")
            .join(observed.cgroup.trim_start_matches('/'))
            .try_exists()
            .map(|exists| !exists)
            .map_err(|e| e.to_string())
    }
}
