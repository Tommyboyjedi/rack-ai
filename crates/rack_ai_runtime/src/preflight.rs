use crate::{
    config::{Config, Driver, Profile},
    process,
    types::*,
};
use rack_ai_media::{command::run, gpu_processes};
pub struct Preflight<'a> {
    pub config: &'a Config,
}
impl Preflight<'_> {
    pub fn check(&self, d: &Demand, victims: &[Demand]) -> Result<(), String> {
        self.check_with_resident_processes(d, victims, &[])
    }

    pub fn check_with_resident_processes(
        &self,
        d: &Demand,
        victims: &[Demand],
        resident_processes: &[Process],
    ) -> Result<(), String> {
        rack_ai_infrastructure::endpoint_fence::EndpointFence::quiescent(
            &self.config.authority_root,
            &d.profile.endpoint,
        )?;
        for victim in victims {
            rack_ai_infrastructure::endpoint_fence::EndpointFence::quiescent(
                &self.config.authority_root,
                &victim.profile.endpoint,
            )?;
        }
        if digest(&std::fs::read(&d.profile.executable).map_err(|e| e.to_string())?)
            != d.profile.executable_sha256
        {
            return Err("executable_hash_mismatch".into());
        }
        if let Some(path) = &d.profile.artifact {
            if !path.is_file() {
                return Err("artifact_missing".into());
            }
            // Full artifact qualification is an administrator operation; a normal load
            // rechecks the pinned digest through bounded sha256sum before any stop.
            let hash = rack_ai_media::command::run_bounded(rack_ai_media::command::CommandSpec {
                program: "sha256sum",
                args: &[path.to_str().ok_or("invalid_artifact_path")?],
                seconds: d.profile.artifact_verify_seconds,
            })?;
            if hash.split_whitespace().next() != d.profile.artifact_sha256.as_deref() {
                return Err("artifact_hash_mismatch".into());
            }
        }
        if d.profile.driver == Driver::Fixture {
            return Ok(());
        }
        if d.profile.backend == crate::config::Backend::Comfyui {
            crate::media_limits::verify(d)?;
        }
        let allowed = victims
            .iter()
            .filter_map(|v| v.process.as_ref())
            .map(process::pids)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let resident_allowed = resident_processes
            .iter()
            .map(process::pids)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten();
        let allowed = allowed
            .into_iter()
            .chain(resident_allowed)
            .collect::<Vec<_>>();
        for resource in &d.profile.resources {
            if self
                .gpu_pids(resource)?
                .iter()
                .any(|p| !allowed.contains(p))
            {
                return Err("foreign_gpu_process".into());
            }
        }
        MemoryProbe {
            config: self.config,
        }
        .memory(&d.profile, false)
    }
    pub fn empty(&self, p: &Profile) -> Result<(), String> {
        self.reclaimed(p)?;
        if p.driver == Driver::Fixture {
            return Ok(());
        }
        MemoryProbe {
            config: self.config,
        }
        .memory(p, true)
    }
    /// Shutdown proves allocations are gone; capacity is only an activation condition.
    pub fn reclaimed(&self, p: &Profile) -> Result<(), String> {
        if p.driver == Driver::Fixture {
            return Ok(());
        }
        for resource in &p.resources {
            if !self.gpu_pids(resource)?.is_empty() {
                return Err("gpu_cleanup_uncertain".into());
            }
        }
        Ok(())
    }
    fn gpu_pids(&self, id: &str) -> Result<Vec<u32>, String> {
        let uuid = &self.config.devices.get(id).ok_or("unknown_resource")?.uuid;
        gpu_processes::parse(&run("nvidia-smi", &["-q", "-x", "-i", uuid])?, uuid)
    }
}

struct MemoryProbe<'a> {
    config: &'a Config,
}
impl MemoryProbe<'_> {
    fn memory(&self, p: &Profile, free: bool) -> Result<(), String> {
        let info = std::fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string())?;
        let available = info
            .lines()
            .find(|l| l.starts_with("MemAvailable:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .ok_or("host_memory_unknown")?
            .parse::<u64>()
            .map_err(|_| "host_memory_unknown")?
            / 1024;
        if available < p.host_mib {
            return Err("insufficient_host_memory".into());
        }
        for (resource, required) in &p.device_mib {
            let uuid = &self.config.devices[resource].uuid;
            let query = if free {
                "--query-gpu=memory.free"
            } else {
                "--query-gpu=memory.total"
            };
            let value = run(
                "nvidia-smi",
                &["-i", uuid, query, "--format=csv,noheader,nounits"],
            )?;
            if value
                .trim()
                .parse::<u64>()
                .map_err(|_| "device_memory_unknown")?
                < *required
            {
                return Err("insufficient_device_memory".into());
            }
        }
        Ok(())
    }
}
