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
        let bytes = std::fs::read(&p.executable).map_err(|e| e.to_string())?;
        if digest(&bytes) != p.executable_sha256 {
            return Err("executable_hash_mismatch".into());
        }
        if p.driver == Driver::Fixture {
            let mut child = Command::new(&p.executable)
                .args(&p.args)
                .env("RACK_RUNTIME_ACTIVATION", &d.generation)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            let observed = process::capture(child.id(), &d.generation);
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return observed;
        }
        let unit = format!("rack-runtime-{}.service", d.generation);
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
            format!("--setenv=RACK_RUNTIME_ACTIVATION={}", d.generation),
            "--".into(),
            p.executable.to_string_lossy().into_owned(),
        ];
        args.extend(p.args.clone());
        run(
            "systemd-run",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )?;
        let observed = rack_ai_media::systemd::Systemd { unit: unit.clone() }.observe()?;
        let mut result = process::capture(observed.pid, &d.generation)?;
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
        if process::gone(process)? {
            return self.released(d);
        }
        process::verify(process, &d.profile)?;
        if let Some(unit) = &process.unit {
            let observed = rack_ai_media::systemd::Systemd { unit: unit.clone() }.observe()?;
            if process.invocation.as_ref() != Some(&observed.invocation) {
                return Err("systemd_invocation_changed".into());
            }
            run("systemctl", &["--user", "stop", "--no-block", unit])?;
        } else {
            run("/bin/kill", &["-TERM", &process.pid.to_string()])?;
        }
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(d.profile.stop_seconds);
        while !process::gone(process)? {
            if std::time::Instant::now() >= deadline {
                return Err("stop_deadline_cleanup_unproven".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.released(d)
    }
    pub fn released(&self, d: &Demand) -> Result<(), String> {
        if let Some(p) = &d.process {
            if !process::gone(p)? {
                return Err("process_still_alive".into());
            }
            if let Some(unit) = &p.unit
                && !(rack_ai_media::systemd::Systemd { unit: unit.clone() }).gone()?
            {
                return Err("cgroup_still_populated".into());
            }
        }
        crate::preflight::Preflight {
            config: self.config,
        }
        .empty(&d.profile)
    }
}
