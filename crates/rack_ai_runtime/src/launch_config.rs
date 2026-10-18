use crate::config::{Backend, Driver, Profile};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};
const MAX_PINS: usize = 512;
const MAX_PIN_PATH_BYTES: usize = 64 * 1024;
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Launch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<PathBuf>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cpu_affinity: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub file_pins: Vec<FilePin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_pin_verifier: Option<PathBuf>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FilePin {
    pub path: PathBuf,
    pub sha256: String,
}
pub fn validate(p: &Profile) -> Result<(), String> {
    if p.enforce_input_limits && !p.input_token_preflight {
        return Err("input enforcement requires input preflight".into());
    }
    if p.input_token_preflight
        && (!matches!(p.backend, Backend::LlamaCpp | Backend::Strata)
            || !p
                .protocols
                .contains(&crate::protocol::Protocol::ChatCompletions))
    {
        return Err("input preflight requires qualified text chat backend".into());
    }
    if p.context_reserve_tokens >= p.context_tokens
        || u64::from(p.max_input_tokens) + u64::from(p.context_reserve_tokens)
            > u64::from(p.context_tokens)
    {
        return Err("invalid context reserve".into());
    }
    if p.backend == Backend::Strata
        && (p.protocols != vec![crate::protocol::Protocol::ChatCompletions]
            || p.launch.as_ref().is_none_or(|l| l.file_pins.is_empty()))
    {
        return Err("strata requires pinned launch and chat_completions only".into());
    }
    if let Some(launch) = &p.launch {
        if p.driver == Driver::Docker {
            return Err("launch controls require systemd or fixture".into());
        }
        validate_launch(launch)?;
    }
    Ok(())
}
fn validate_launch(l: &Launch) -> Result<(), String> {
    let unique: BTreeSet<_> = l.cpu_affinity.iter().collect();
    if unique.len() != l.cpu_affinity.len()
        || l.cpu_affinity.iter().any(|cpu| *cpu > 4095)
        || l.working_directory.as_ref().is_some_and(|p| !absolute(p))
        || l.file_pin_verifier
            .as_ref()
            .is_some_and(|p| !absolute(p) || p.to_str().is_none())
        || l.environment.len() > 32
        || l.environment.iter().any(|(k, v)| !environment(k, v))
        || l.file_pins.len() > MAX_PINS
        || l.file_pins
            .iter()
            .map(|f| f.path.as_os_str().len())
            .sum::<usize>()
            > MAX_PIN_PATH_BYTES
    {
        return Err("invalid launch controls".into());
    }
    let unique: BTreeSet<_> = l.file_pins.iter().map(|f| &f.path).collect();
    if unique.len() != l.file_pins.len()
        || l.file_pins.iter().any(|f| {
            !absolute(&f.path)
                || f.sha256.len() != 64
                || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        })
    {
        return Err("invalid launch file pins".into());
    }
    Ok(())
}
fn absolute(p: &Path) -> bool {
    p.is_absolute()
        && !p
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        && !p.to_string_lossy().contains(['\n', '\r', '\0'])
}
fn environment(key: &str, value: &str) -> bool {
    ![
        "CUDA_VISIBLE_DEVICES",
        "NVIDIA_VISIBLE_DEVICES",
        "RACK_RUNTIME_ACTIVATION",
    ]
    .contains(&key)
        && !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_uppercase() || b == b'_')
        && key
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && value.len() <= 8192
        && !value.contains(['\n', '\r', '\0'])
}
pub fn systemd_arguments(p: &Profile) -> Vec<String> {
    let Some(l) = &p.launch else {
        return Vec::new();
    };
    let mut args = Vec::new();
    if let Some(cwd) = &l.working_directory {
        args.push(format!("--property=WorkingDirectory={}", cwd.display()));
    }
    if !l.cpu_affinity.is_empty() {
        let mask = l
            .cpu_affinity
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        args.push(format!("--property=CPUAffinity={mask}"));
    }
    args.extend(
        l.environment
            .iter()
            .map(|(k, v)| format!("--setenv={k}={v}")),
    );
    args
}
