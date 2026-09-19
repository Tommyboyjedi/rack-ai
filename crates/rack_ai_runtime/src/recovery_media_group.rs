//! Attribution of remaining descendants when the media main process has exited.
use crate::types::{Demand, Process};
use rack_ai_media::{runtime::Runtime, systemd::Observation};
use std::{fs, path::Path};

pub struct GroupCleanup<'a> {
    pub runtime: &'a Runtime,
}
impl GroupCleanup<'_> {
    pub fn process(&self, d: &Demand, observed: &Observation) -> Result<Option<Process>, String> {
        let media = self.runtime.store.read()?.service;
        if observed.pid != 0
            || observed.pending_job
            || observed.invocation.is_empty()
            || observed.cgroup.is_empty()
            || media.activation != d.generation
            || media.invocation.as_ref() != Some(&observed.invocation)
            || media
                .lease
                .as_ref()
                .is_none_or(|lease| lease.owner != d.id || lease.generation != d.generation)
        {
            return Err("media_recovery_group_binding_unproven".into());
        }
        if let Some(saved) = &d.process
            && (saved.activation != d.generation
                || saved.unit.as_ref() != Some(&self.runtime.config.unit)
                || saved.invocation.as_ref() != Some(&observed.invocation)
                || !crate::process::gone(saved)?)
        {
            return Err("media_recovery_group_process_changed".into());
        }
        if let Some(generation) = &media.generation {
            require_generation(generation, observed)?;
            require_main_gone(generation)?;
        } else if d.process.is_none() {
            return Err("media_recovery_group_generation_missing".into());
        }
        if let Some(restart) = &media.restart {
            for generation in std::iter::once(&restart.previous).chain(restart.target.as_ref()) {
                // Any other retained generation must also be gone before cleanup
                // can establish that this unit owns all remaining effects.
                require_main_gone(generation)?;
            }
        }
        let cgroup = Path::new("/sys/fs/cgroup").join(observed.cgroup.trim_start_matches('/'));
        let members = fs::read_to_string(cgroup.join("cgroup.procs")).map_err(|e| e.to_string())?;
        let events = fs::read_to_string(cgroup.join("cgroup.events")).map_err(|e| e.to_string())?;
        if !events
            .lines()
            .any(|line| matches!(line, "populated 0" | "populated 1"))
        {
            return Err("media_recovery_group_state_unreadable".into());
        }
        let mut pids = members
            .lines()
            .map(|line| {
                line.parse::<u32>()
                    .map_err(|_| "media_recovery_group_pid_invalid".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        pids.extend(
            (rack_ai_media::gpu::GpuProbe {
                config: &self.runtime.config,
            })
            .pids()?,
        );
        for pid in pids {
            let groups =
                fs::read_to_string(format!("/proc/{pid}/cgroup")).map_err(|e| e.to_string())?;
            if !groups
                .lines()
                .filter_map(|line| line.split_once("::"))
                .any(|(_, group)| Path::new(group).starts_with(&observed.cgroup))
            {
                return Err("media_recovery_group_foreign_process".into());
            }
        }
        // The caller stops the verified invocation and waits for the entire unit
        // and GPU allocation to disappear; this receipt never invents a new PID.
        Ok(d.process.clone())
    }
}

fn require_generation(
    generation: &rack_ai_media::backend_generation::BackendGeneration,
    observed: &Observation,
) -> Result<(), String> {
    if generation.invocation != observed.invocation || generation.cgroup != observed.cgroup {
        return Err("media_recovery_group_generation_changed".into());
    }
    Ok(())
}

pub(super) fn require_main_gone(
    generation: &rack_ai_media::backend_generation::BackendGeneration,
) -> Result<(), String> {
    if rack_ai_media::backend_generation::present_start_ticks(generation.pid)?
        == Some(generation.start_ticks)
    {
        return match crate::process::capture(generation.pid, &generation.invocation) {
            Err(error) if error == "process_is_zombie" => Ok(()),
            Err(error) => Err(error),
            Ok(_) => Err("media_recovery_group_previous_process_alive".into()),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exited_zombie_is_not_a_live_owned_effect() {
        let mut child = std::process::Command::new("/usr/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let process = crate::process::capture(child.id(), "fixture").unwrap();
        let generation = rack_ai_media::backend_generation::BackendGeneration {
            number: 1,
            invocation: "owned".into(),
            pid: child.id(),
            cgroup: "/fixture/owned".into(),
            start_ticks: process.start.parse().unwrap(),
        };
        let live = require_main_gone(&generation);
        child.kill().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let mut zombie = Err("child did not become zombie".to_string());
        while std::time::Instant::now() < deadline {
            if crate::process::capture(child.id(), "fixture")
                .is_err_and(|e| e == "process_is_zombie")
            {
                zombie = require_main_gone(&generation);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        child.wait().unwrap();
        assert_eq!(
            live.unwrap_err(),
            "media_recovery_group_previous_process_alive"
        );
        assert!(zombie.is_ok(), "{zombie:?}");
        assert!(require_main_gone(&generation).is_ok());
    }
    #[test]
    fn descendant_group_requires_exact_recorded_invocation_and_cgroup() {
        let generation = rack_ai_media::backend_generation::BackendGeneration {
            number: 1,
            invocation: "owned".into(),
            pid: 7,
            cgroup: "/fixture/owned".into(),
            start_ticks: 12,
        };
        let mut observed = Observation {
            invocation: "owned".into(),
            pid: 0,
            cgroup: "/fixture/owned".into(),
            active: "deactivating".into(),
            pending_job: false,
        };
        assert!(require_generation(&generation, &observed).is_ok());
        observed.invocation = "foreign".into();
        assert!(require_generation(&generation, &observed).is_err());
        observed.invocation = "owned".into();
        observed.cgroup = "/fixture/owned-other".into();
        assert!(require_generation(&generation, &observed).is_err());
    }
}
