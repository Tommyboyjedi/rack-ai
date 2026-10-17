use crate::{
    config::{Config, Driver, Profile},
    types::{Demand, Process},
};
use rack_ai_media::{command::run, gpu_processes};
use std::{collections::BTreeSet, fs};
pub fn verify(process: &Process, profile: &Profile) -> Result<(), String> {
    let Some(launch) = &profile.launch else {
        return Ok(());
    };
    let root = format!("/proc/{}", process.pid);
    let raw = fs::read(format!("{root}/cmdline")).map_err(|_| "launch_identity_unavailable")?;
    let actual = raw.split(|b| *b == 0).skip(1).collect::<Vec<_>>();
    let expected = profile
        .args
        .iter()
        .map(|arg| arg.as_bytes())
        .chain(std::iter::once(&b""[..]))
        .collect::<Vec<_>>();
    if actual != expected {
        return Err("launch_arguments_mismatch".into());
    }
    if let Some(cwd) = &launch.working_directory {
        if fs::read_link(format!("{root}/cwd")).map_err(|_| "launch_identity_unavailable")?
            != cwd
                .canonicalize()
                .map_err(|_| "launch_identity_unavailable")?
        {
            return Err("launch_directory_mismatch".into());
        }
    }
    let env = fs::read(format!("{root}/environ")).map_err(|_| "launch_identity_unavailable")?;
    for (key, value) in &launch.environment {
        require_environment(&env, (key, value))?;
    }
    if !launch.cpu_affinity.is_empty() {
        let status = fs::read_to_string(format!("{root}/status"))
            .map_err(|_| "launch_identity_unavailable")?;
        let mask = status
            .lines()
            .find_map(|l| l.strip_prefix("Cpus_allowed_list:"))
            .ok_or("launch_affinity_unavailable")?;
        if cpu_set(mask.trim())? != launch.cpu_affinity.iter().copied().collect() {
            return Err("launch_affinity_mismatch".into());
        }
    }
    Ok(())
}
pub fn devices(config: &Config, demand: &Demand) -> Result<(), String> {
    if demand.profile.driver == Driver::Fixture {
        return Ok(());
    }
    let process = demand
        .process
        .as_ref()
        .ok_or("activation_process_missing")?;
    let visible = demand
        .profile
        .resources
        .iter()
        .map(|id| config.devices[id].uuid.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let env = fs::read(format!("/proc/{}/environ", process.pid))
        .map_err(|_| "launch_identity_unavailable")?;
    require_environment(&env, ("CUDA_VISIBLE_DEVICES", &visible))?;
    let owned = crate::process::pids(process)?;
    for (id, device) in &config.devices {
        let actual = gpu_processes::parse(
            &run("nvidia-smi", &["-q", "-x", "-i", &device.uuid])?,
            &device.uuid,
        )?;
        let required = demand.profile.resources.contains(id);
        if (required && (actual.is_empty() || actual.iter().any(|p| !owned.contains(p))))
            || (!required && actual.iter().any(|p| owned.contains(p)))
        {
            return Err("launch_gpu_allocation_unproven".into());
        }
    }
    Ok(())
}
fn require_environment(raw: &[u8], input: (&str, &str)) -> Result<(), String> {
    let (key, value) = input;
    let prefix = format!("{key}=");
    let expected = format!("{prefix}{value}");
    let entries = raw
        .split(|b| *b == 0)
        .filter(|e| e.starts_with(prefix.as_bytes()))
        .collect::<Vec<_>>();
    if entries != vec![expected.as_bytes()] {
        return Err("launch_environment_mismatch".into());
    }
    Ok(())
}
fn cpu_set(mask: &str) -> Result<BTreeSet<u32>, String> {
    let mut result = BTreeSet::new();
    for segment in mask.split(',') {
        let (start, end) = segment.split_once('-').unwrap_or((segment, segment));
        let start: u32 = start.parse().map_err(|_| "launch_affinity_unavailable")?;
        let end: u32 = end.parse().map_err(|_| "launch_affinity_unavailable")?;
        if start > end || end > 4095 {
            return Err("launch_affinity_unavailable".into());
        }
        result.extend(start..=end);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    #[test]
    fn affinity_parses_disjoint_kernel_ranges() {
        assert_eq!(super::cpu_set("0-2,7").unwrap(), [0, 1, 2, 7].into());
        assert!(super::cpu_set("9-2").is_err());
        assert!(super::cpu_set("0-999999").is_err());
    }
}
