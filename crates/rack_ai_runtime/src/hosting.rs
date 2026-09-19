use crate::{
    config::{Config, Driver},
    process,
    types::*,
};
use rack_ai_media::command::run;
use std::process::{Command, Stdio};
pub struct Hosting<'a> {
    pub config: &'a Config,
}
impl Hosting<'_> {
    pub fn start(&self, d: &Demand) -> Result<Process, String> {
        if d.profile.backend == crate::config::Backend::Comfyui
            && d.profile.driver != Driver::Fixture
        {
            return crate::media::MediaAdapter {
                config: self.config,
            }
            .start(d);
        }
        if d.profile.driver == Driver::Docker {
            return crate::container::ContainerHosting {
                config: self.config,
            }
            .start(d);
        }
        let p = &d.profile;
        let activation = d.backend_activation();
        let bytes = std::fs::read(&p.executable).map_err(|e| e.to_string())?;
        if digest(&bytes) != p.executable_sha256 {
            return Err("executable_hash_mismatch".into());
        }
        if p.driver == Driver::Fixture {
            let mut child = Command::new(&p.executable)
                .args(&p.args)
                .env("RACK_RUNTIME_ACTIVATION", activation)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            let observed = process::capture(child.id(), activation);
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return observed;
        }
        let unit = format!("rack-runtime-{}.service", activation);
        let devices = p
            .resources
            .iter()
            .map(|r| self.config.devices[r].uuid.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let mut args = vec![
            "--user".to_string(),
            format!("--unit={unit}"),
            "--collect".into(),
            "--property=Type=exec".into(),
            "--property=KillMode=control-group".into(),
            "--property=Restart=no".into(),
            "--property=MemorySwapMax=0".into(),
            format!("--property=MemoryMax={}M", p.host_mib),
            format!("--property=CPUQuota={}%", p.cpu_percent),
            format!("--property=TimeoutStopSec={}", p.stop_seconds),
            format!("--setenv=CUDA_VISIBLE_DEVICES={devices}"),
            format!("--setenv=RACK_RUNTIME_ACTIVATION={}", activation),
            "--".into(),
            p.executable.to_string_lossy().into_owned(),
        ];
        args.extend(p.args.clone());
        run(
            "systemd-run",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )?;
        let observed = rack_ai_media::systemd::Systemd { unit: unit.clone() }.observe()?;
        let mut result = process::capture(observed.pid, activation)?;
        result.unit = Some(unit);
        result.invocation = Some(observed.invocation);
        Ok(result)
    }
    pub fn stop(&self, d: &Demand) -> Result<(), String> {
        HostedCleanup {
            config: self.config,
        }
        .stop(d)
    }
}
pub struct HostedCleanup<'a> {
    pub config: &'a Config,
}
impl HostedCleanup<'_> {
    pub fn stop(&self, d: &Demand) -> Result<(), String> {
        if d.process.is_none() && !d.effect_started {
            return Ok(());
        }
        if d.profile.backend == crate::config::Backend::Comfyui
            && d.profile.driver != Driver::Fixture
        {
            return crate::media::MediaAdapter {
                config: self.config,
            }
            .stop(d);
        }
        if d.profile.driver == Driver::Docker {
            return crate::container::ContainerHosting {
                config: self.config,
            }
            .stop(d);
        }
        let Some(process) = &d.process else {
            return Ok(());
        };
        if process.activation != d.backend_activation() {
            return Err("activation_mismatch".into());
        }
        let gone = process::gone(process)?;
        if !gone {
            process::verify(process, &d.profile)?;
        }
        if let Some(unit) = &process.unit {
            if unit != &format!("rack-runtime-{}.service", process.activation) {
                return Err("systemd_activation_changed".into());
            }
            let observed = rack_ai_media::systemd::Systemd { unit: unit.clone() }.observe()?;
            if gone
                && observed.invocation.is_empty()
                && observed.pid == 0
                && !observed.pending_job
                && matches!(observed.active.as_str(), "inactive" | "failed")
            {
                return self.released(d);
            }
            if process.invocation.as_deref().is_none_or(str::is_empty)
                || process.invocation.as_ref() != Some(&observed.invocation)
            {
                return Err("systemd_invocation_changed".into());
            }
            if observed.pid != process.pid && !(gone && observed.pid == 0) {
                return Err("systemd_process_changed".into());
            }
            run("systemctl", &["--user", "stop", "--no-block", unit])?;
        } else {
            if gone {
                return self.released(d);
            }
            run("/bin/kill", &["-TERM", &process.pid.to_string()])?;
        }
        self.released(d)
    }
    pub fn released(&self, d: &Demand) -> Result<(), String> {
        if d.process.is_some() {
            crate::teardown::Teardown::wait(d)?;
        }
        crate::gpu_cleanup::wait(self.config, &d.profile)
    }
}
