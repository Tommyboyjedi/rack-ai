use crate::{
    process,
    types::{Demand, Process},
};
use serde_json::Value;
use std::{fs, path::PathBuf};
pub(super) mod pinned;
use pinned::EngineConfig;

pub fn verify(demand: &Demand, status: &Value) -> Result<Process, String> {
    let parent = demand
        .process
        .as_ref()
        .ok_or("activation_process_missing")?;
    if parent.unit.is_none() {
        return Err("strata_owned_unit_required".into());
    }
    let config = EngineConfig::load(demand, status)?;
    let engine_path = config
        .exe
        .canonicalize()
        .map_err(|_| "strata_engine_unreadable")?;
    let pid = status["engine_pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid != 0)
        .ok_or("strata_engine_pid_missing")?;
    if !process::pids(parent)?.contains(&pid) {
        return Err("strata_engine_not_owned".into());
    }
    let root = PathBuf::from(format!("/proc/{pid}"));
    let parent_root = PathBuf::from(format!("/proc/{}", parent.pid));
    if fs::read_link(root.join("exe")).map_err(|_| "strata_engine_missing")? != engine_path
        || fs::read_link(root.join("cwd")).map_err(|_| "strata_engine_cwd_missing")? != config.cwd
        || fs::read(root.join("cgroup")).map_err(|_| "strata_engine_cgroup_missing")?
            != fs::read(parent_root.join("cgroup")).map_err(|_| "strata_parent_cgroup_missing")?
    {
        return Err("strata_engine_identity_mismatch".into());
    }
    let child_status =
        fs::read_to_string(root.join("status")).map_err(|_| "strata_engine_status_missing")?;
    if !child_status.lines().any(|line| {
        line.strip_prefix("PPid:")
            .is_some_and(|p| p.trim() == parent.pid.to_string())
    }) {
        return Err("strata_engine_parent_mismatch".into());
    }
    let raw = fs::read(root.join("cmdline")).map_err(|_| "strata_engine_cmdline_missing")?;
    let observed: Vec<_> = raw.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
    let mut expected = vec![b"--serve".as_slice()];
    expected.extend(config.args.iter().map(|v| v.as_bytes()));
    if observed.get(1..) != Some(expected.as_slice()) {
        return Err("strata_engine_arguments_mismatch".into());
    }
    let env = fs::read(root.join("environ")).map_err(|_| "strata_engine_environment_missing")?;
    let parent_env =
        fs::read(parent_root.join("environ")).map_err(|_| "strata_parent_environment_missing")?;
    let cuda = parent_env
        .split(|b| *b == 0)
        .find(|v| v.starts_with(b"CUDA_VISIBLE_DEVICES="))
        .ok_or("strata_parent_devices_missing")?;
    if !env.split(|b| *b == 0).any(|v| v == cuda) {
        return Err("strata_engine_devices_mismatch".into());
    }
    for (key, value) in config.env {
        let expected = format!("{key}={value}");
        if !env.split(|b| *b == 0).any(|v| v == expected.as_bytes()) {
            return Err("strata_engine_environment_mismatch".into());
        }
    }
    process::capture(pid, &parent.activation)
}
