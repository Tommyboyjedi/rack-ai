use crate::{command::run, config::Config, types::Service};
#[derive(Clone)]
pub struct Systemd {
    pub unit: String,
}
pub struct Observation {
    pub invocation: String,
    pub pid: u32,
    pub cgroup: String,
    pub active: String,
}
impl Systemd {
    pub fn new(config: &Config) -> Self {
        Self {
            unit: config.unit.clone(),
        }
    }
    pub fn observe(&self) -> Result<Observation, String> {
        let text = run(
            "systemctl",
            &[
                "--user",
                "show",
                &self.unit,
                "-p",
                "InvocationID",
                "-p",
                "MainPID",
                "-p",
                "ControlGroup",
                "-p",
                "ActiveState",
            ],
        )?;
        let values: std::collections::BTreeMap<_, _> =
            text.lines().filter_map(|l| l.split_once('=')).collect();
        Ok(Observation {
            invocation: values.get("InvocationID").unwrap_or(&"").to_string(),
            pid: values
                .get("MainPID")
                .unwrap_or(&"")
                .parse()
                .map_err(|_| "invalid service PID")?,
            cgroup: values.get("ControlGroup").unwrap_or(&"").to_string(),
            active: values.get("ActiveState").unwrap_or(&"").to_string(),
        })
    }
    pub fn start(&self) -> Result<(), String> {
        let observed = self.observe()?;
        if observed.pid != 0
            || !observed.invocation.is_empty()
            || !matches!(observed.active.as_str(), "inactive" | "failed")
        {
            return Err("owned unit is already active or ambiguous".into());
        }
        run("systemctl", &["--user", "start", "--no-block", &self.unit]).map(|_| ())
    }
    pub fn stop(&self, expected: &Service) -> Result<(), String> {
        let observed = self.observe()?;
        if observed.pid == 0 && observed.active == "inactive" {
            return Ok(());
        }
        if expected.invocation.as_ref() != Some(&observed.invocation)
            || observed.invocation.is_empty()
        {
            return Err("refusing stop: service invocation changed".into());
        }
        run("systemctl", &["--user", "stop", "--no-block", &self.unit]).map(|_| ())
    }
    pub fn gone(&self) -> Result<bool, String> {
        let o = self.observe()?;
        if o.pid != 0 || !matches!(o.active.as_str(), "inactive" | "failed") {
            return Ok(false);
        }
        if o.cgroup.is_empty() {
            return Ok(true);
        }
        let path = std::path::Path::new("/sys/fs/cgroup").join(o.cgroup.trim_start_matches('/'));
        if !path.exists() {
            return Ok(true);
        }
        let events =
            std::fs::read_to_string(path.join("cgroup.events")).map_err(|e| e.to_string())?;
        Ok(events.lines().any(|l| l == "populated 0"))
    }
}
