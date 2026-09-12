use crate::{command::run, config::Config, systemd::Observation};
pub struct GpuProbe<'a> {
    pub config: &'a Config,
}
impl GpuProbe<'_> {
    pub fn placement(&self) -> Result<(), String> {
        crate::placement::PlacementProbe {
            config: self.config,
        }
        .verify()
    }
    pub fn pids(&self) -> Result<Vec<u32>, String> {
        let raw = run(
            "nvidia-smi",
            &[
                "--query-compute-apps=gpu_uuid,pid",
                "--format=csv,noheader,nounits",
            ],
        )?;
        let mut pids = vec![];
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            let (uuid, pid) = line.split_once(',').ok_or("ambiguous GPU process probe")?;
            if uuid.trim() == self.config.media_uuid {
                pids.push(pid.trim().parse().map_err(|_| "invalid GPU PID")?);
            }
        }
        Ok(pids)
    }
    pub fn owned(&self, observed: &Observation) -> Result<(), String> {
        if observed.pid == 0 || observed.invocation.is_empty() || observed.cgroup.is_empty() {
            return Err("missing service identity".into());
        }
        for pid in self.pids()?.into_iter().chain([observed.pid]) {
            let cgroups = std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
                .map_err(|_| "process identity disappeared")?;
            if !cgroups
                .lines()
                .filter_map(|l| l.split_once("::"))
                .any(|(_, p)| std::path::Path::new(p).starts_with(&observed.cgroup))
            {
                return Err("foreign process on media GPU".into());
            }
        }
        Ok(())
    }
    pub fn headroom(&self) -> Result<(), String> {
        let memory = std::fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string())?;
        let available = memory
            .lines()
            .find(|l| l.starts_with("MemAvailable:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|n| n.parse::<u64>().ok())
            .ok_or("memory probe failed")?
            / 1024;
        let root = self
            .config
            .output_root
            .to_str()
            .ok_or("invalid output root")?;
        let disk = run("df", &["-Pm", root])?;
        let free = disk
            .lines()
            .nth(1)
            .and_then(|l| l.split_whitespace().nth(3))
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("disk probe failed")?;
        if available < self.config.min_memory_mb || free < self.config.min_disk_mb {
            return Err("workflow host memory/disk headroom unavailable".into());
        }
        Ok(())
    }
}
