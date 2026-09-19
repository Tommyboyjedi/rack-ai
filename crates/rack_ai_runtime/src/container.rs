use crate::{config::Config, types::*};
use rack_ai_media::command::run;
use serde::Deserialize;
use std::collections::BTreeMap;
const STOP_COMMAND_MAX_SECONDS: u64 = 3600;
const STOP_COMMAND_MARGIN_SECONDS: u64 = 2;
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Container {
    id: String,
    image: String,
    state: Status,
    config: ContainerConfig,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Status {
    pid: u32,
    running: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContainerConfig {
    labels: BTreeMap<String, String>,
}
pub struct ContainerHosting<'a> {
    pub config: &'a Config,
}
impl ContainerHosting<'_> {
    pub fn start(&self, d: &Demand) -> Result<Process, String> {
        let activation = d.backend_activation();
        let image = d
            .profile
            .container_image
            .as_ref()
            .ok_or("container_image_missing")?;
        let devices = d
            .profile
            .resources
            .iter()
            .map(|r| self.config.devices[r].uuid.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let mut args = vec![
            "run".into(),
            "--detach".into(),
            "--pull=never".into(),
            "--restart=no".into(),
            format!("--name=rack-runtime-{}", activation),
            format!("--label=rack.activation={}", activation),
            "--network=host".into(),
            "--gpus".into(),
            format!("device={devices}"),
            "--pids-limit=256".into(),
            format!("--memory={}m", d.profile.host_mib),
            format!("--memory-swap={}m", d.profile.host_mib),
            format!("--cpus={}", d.profile.cpu_percent as f64 / 100.0),
            "--env=HF_HUB_OFFLINE=1".into(),
            "--env=TRANSFORMERS_OFFLINE=1".into(),
            format!("--env=RACK_RUNTIME_ACTIVATION={}", activation),
        ];
        for (source, target) in &d.profile.container_mounts {
            args.extend([
                "--mount".into(),
                format!("type=bind,src={},dst={target},readonly", source.display()),
            ]);
        }
        args.push(image.clone());
        args.extend(d.profile.args.clone());
        let id = run(
            d.profile
                .executable
                .to_str()
                .ok_or("invalid_container_executable")?,
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )?
        .trim()
        .to_string();
        let observed = inspect(&id, &d.profile.executable)?;
        if !observed.state.running
            || observed.image != *image
            || observed
                .config
                .labels
                .get("rack.activation")
                .map(String::as_str)
                != Some(activation)
        {
            return Err("container_start_identity_unproven".into());
        }
        let mut p = crate::process::capture(observed.state.pid, activation)?;
        p.container = Some(id);
        Ok(p)
    }
    pub fn stop(&self, d: &Demand) -> Result<(), String> {
        let Some(p) = &d.process else {
            return Ok(());
        };
        let id = p.container.as_ref().ok_or("container_identity_missing")?;
        verify(p, &d.profile)?;
        if inspect(id, &d.profile.executable)?.state.running {
            let grace = d
                .profile
                .stop_seconds
                .min(STOP_COMMAND_MAX_SECONDS - STOP_COMMAND_MARGIN_SECONDS);
            rack_ai_media::command::run_bounded(rack_ai_media::command::CommandSpec {
                program: d
                    .profile
                    .executable
                    .to_str()
                    .ok_or("invalid_container_executable")?,
                args: &["stop", "--time", &grace.to_string(), id],
                seconds: grace + STOP_COMMAND_MARGIN_SECONDS,
            })?;
        }
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(d.profile.stop_seconds);
        while inspect(id, &d.profile.executable)?.state.running {
            if std::time::Instant::now() >= deadline {
                return Err("container_cleanup_unproven".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !crate::process::gone(p)? {
            return Err("container_process_still_alive".into());
        }
        crate::gpu_cleanup::wait(self.config, &d.profile)
    }
}
fn inspect(id: &str, executable: &std::path::Path) -> Result<Container, String> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid_container_id".into());
    }
    let raw = run(
        executable.to_str().ok_or("invalid_container_executable")?,
        &["inspect", id],
    )?;
    let mut values: Vec<Container> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if values.len() != 1 {
        return Err("ambiguous_container_identity".into());
    }
    values.pop().ok_or("container_missing".into())
}
pub fn verify(p: &Process, profile: &crate::config::Profile) -> Result<(), String> {
    let id = p.container.as_ref().ok_or("container_identity_missing")?;
    let observed = inspect(id, &profile.executable)?;
    if observed.id != *id
        || profile.container_image.as_ref() != Some(&observed.image)
        || observed.config.labels.get("rack.activation") != Some(&p.activation)
        || (observed.state.running && observed.state.pid != p.pid)
    {
        return Err("container_generation_mismatch".into());
    }
    Ok(())
}
