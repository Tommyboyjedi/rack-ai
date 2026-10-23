//! Optional, hash-pinned local operator bridge, separate from frozen model profiles.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
pub const SCHEMA: &str = "rack-ai/resource-recovery/v1";
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceCommand {
    #[serde(default)]
    pub receiver_guard: Option<ReceiverGuard>,
    pub executable: PathBuf,
    pub sha256: String,
    pub config: PathBuf,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub resources: Vec<Resource>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub resource: String,
    pub services: Vec<String>,
    pub status: ResourceStatus,
}
impl ResourceCommand {
    pub fn load(root: &Path) -> Result<Option<Self>, String> {
        let path = root.join("resource-recovery.json");
        if !path.exists() {
            return Ok(None);
        }
        private_file(&path)?;
        let command: Self = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|_| "invalid resource recovery configuration")?;
        if !command.executable.is_absolute()
            || !command.config.is_absolute()
            || command.sha256.len() != 64
        {
            return Err("invalid resource recovery binding".into());
        }
        Ok(Some(command))
    }
    pub fn execute(&self, apply: bool) -> Result<Report, String> {
        if let Some(guard) = &self.receiver_guard {
            guard.verify()?;
        }
        private_file(&self.config)?;
        let binary =
            fs::read(&self.executable).map_err(|_| "resource recovery helper unavailable")?;
        if crate::types::digest(&binary) != self.sha256 {
            return Err("resource recovery helper identity changed".into());
        }
        let executable = self.executable.to_str().ok_or("invalid helper path")?;
        let config = self.config.to_str().ok_or("invalid helper config")?;
        let operation = if apply { "recover" } else { "inspect" };
        let output = crate::command::run_bounded(crate::command::CommandSpec {
            program: executable,
            args: &[operation, config],
            seconds: 20,
        })
        .map_err(|_| "resource recovery could not verify completion; refresh resource status")?;
        let report: Report =
            serde_json::from_str(&output).map_err(|_| "invalid resource recovery report")?;
        if report.schema != SCHEMA || report.resources.len() > 64 {
            return Err("invalid resource recovery report".into());
        }
        Ok(report)
    }
}
fn private_file(path: &Path) -> Result<(), String> {
    if fs::metadata(path)
        .map_err(|_| "resource recovery configuration unavailable")?
        .permissions()
        .mode()
        & 0o077
        != 0
    {
        return Err("resource recovery configuration must be private".into());
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStatus {
    Available,
    InUsePreserved,
    ActiveWorkPreserved,
    UnresolvedWorkPreserved,
    WarmPreserved,
    CleanupPending,
    CleanupRequested,
    ResetAvailable,
    ResetComplete,
    MediaSessionPreserved,
    BlockedUnprovenOwner,
    BlockedMediaBinding,
    BlockedPhysicalVerification,
    BlockedLiveOrUnprovenProcess,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverGuard {
    pub sha256: String,
}
impl ReceiverGuard {
    fn verify(&self) -> Result<(), String> {
        let output = crate::command::run(
            "systemctl",
            &[
                "--user",
                "show",
                "rack-ai-runtime.service",
                "-p",
                "MainPID",
                "--value",
            ],
        )?;
        let pid: u32 = output
            .trim()
            .parse()
            .map_err(|_| "runtime identity unavailable")?;
        if pid == 0 {
            return Err("runtime receiver is not running".into());
        }
        let binary =
            fs::read(format!("/proc/{pid}/exe")).map_err(|_| "runtime identity unavailable")?;
        if crate::types::digest(&binary) != self.sha256 {
            return Err("runtime changed; operator recovery binding needs requalification".into());
        }
        Ok(())
    }
}
