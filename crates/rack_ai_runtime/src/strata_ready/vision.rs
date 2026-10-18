//! Bind the resident CPU image encoder to the same managed activation.
use super::owned::pinned::{EngineConfig, VisionConfig};
use crate::{
    process,
    types::{Demand, Process},
};
use serde_json::Value;
use std::{fs, path::PathBuf};

pub fn verify(demand: &Demand, status: &Value) -> Result<Option<Process>, String> {
    let config = EngineConfig::load(demand, status)?;
    let Some(vision) = config.vision else {
        if demand.profile.max_images_per_request > 0 || status["vision"]["enabled"] == true {
            return Err("strata_vision_configuration_missing".into());
        }
        return Ok(None);
    };
    if demand.profile.max_images_per_request == 0
        || status["vision"]["enabled"] != true
        || status["vision"]["loaded"] != true
        || status["vision"]["protocol"] != "rackai-count-v1"
        || vision.gpu
        || vision.threads != 16
        || vision.max_tokens != 1024
    {
        return Err("strata_vision_readiness_mismatch".into());
    }
    let parent = demand
        .process
        .as_ref()
        .ok_or("activation_process_missing")?;
    let pid = status["vision"]["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid != 0)
        .ok_or("strata_vision_pid_missing")?;
    if !process::pids(parent)?.contains(&pid) || pid == parent.pid {
        return Err("strata_vision_not_owned".into());
    }
    let root = PathBuf::from(format!("/proc/{pid}"));
    let parent_root = PathBuf::from(format!("/proc/{}", parent.pid));
    let directory = status["vision"]["directory"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("strata_vision_directory_missing")?;
    let expected_exe = vision
        .exe
        .canonicalize()
        .map_err(|_| "strata_vision_unreadable")?;
    if fs::read_link(root.join("exe")).map_err(|_| "strata_vision_missing")? != expected_exe
        || fs::read_link(root.join("cwd")).map_err(|_| "strata_vision_cwd_missing")? != directory
        || fs::read(root.join("cgroup")).map_err(|_| "strata_vision_cgroup_missing")?
            != fs::read(parent_root.join("cgroup")).map_err(|_| "strata_parent_cgroup_missing")?
    {
        return Err("strata_vision_identity_mismatch".into());
    }
    let child_status =
        fs::read_to_string(root.join("status")).map_err(|_| "strata_vision_status_missing")?;
    if !child_status.lines().any(|line| {
        line.strip_prefix("PPid:")
            .is_some_and(|pid| pid.trim() == parent.pid.to_string())
    }) {
        return Err("strata_vision_parent_mismatch".into());
    }
    verify_arguments(&root, &vision)?;
    let env = fs::read(root.join("environ")).map_err(|_| "strata_vision_environment_missing")?;
    if !env
        .split(|byte| *byte == 0)
        .any(|item| item == b"CUDA_VISIBLE_DEVICES=-1")
    {
        return Err("strata_vision_cpu_placement_mismatch".into());
    }
    Ok(Some(process::capture(pid, &parent.activation)?))
}

fn verify_arguments(root: &std::path::Path, vision: &VisionConfig) -> Result<(), String> {
    let raw = fs::read(root.join("cmdline")).map_err(|_| "strata_vision_cmdline_missing")?;
    let observed: Vec<_> = raw
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .collect();
    let model = vision
        .model
        .to_str()
        .ok_or("strata_vision_model_path_invalid")?;
    let projector = vision
        .mmproj
        .to_str()
        .ok_or("strata_vision_projector_path_invalid")?;
    let expected = [
        "--mmproj",
        projector,
        "--model",
        model,
        "--threads",
        "16",
        "--max-tokens",
        "1024",
    ];
    let expected: Vec<_> = expected.iter().map(|arg| arg.as_bytes()).collect();
    if observed.get(1..) != Some(expected.as_slice()) {
        return Err("strata_vision_arguments_mismatch".into());
    }
    Ok(())
}
