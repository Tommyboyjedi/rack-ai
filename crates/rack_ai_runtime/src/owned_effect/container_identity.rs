use crate::types::Demand;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Container {
    pub id: String,
    name: String,
    image: String,
    pub state: Status,
    config: Configuration,
    host_config: HostConfiguration,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Status {
    pub pid: u32,
    pub running: bool,
    restarting: bool,
    paused: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Configuration {
    labels: BTreeMap<String, String>,
    env: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct HostConfiguration {
    restart_policy: RestartPolicy,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RestartPolicy {
    name: String,
}

impl Container {
    pub fn verify(&self, d: &Demand, id: &str) -> Result<(), String> {
        valid_id(id)?;
        let expected_env = format!("RACK_RUNTIME_ACTIVATION={}", d.generation);
        let activation_env: Vec<_> = self
            .config
            .env
            .iter()
            .filter(|entry| entry.starts_with("RACK_RUNTIME_ACTIVATION="))
            .collect();
        if self.id != id
            || self.name != format!("/rack-runtime-{}", d.generation)
            || d.profile.container_image.as_ref() != Some(&self.image)
            || self.config.labels.get("rack.activation") != Some(&d.generation)
            || activation_env != vec![&expected_env]
            || self.host_config.restart_policy.name != "no"
            || self.state.restarting
            || self.state.paused
            || self.state.running != (self.state.pid != 0)
        {
            return Err("recovery_container_identity_ambiguous".into());
        }
        if let Some(saved) = &d.process
            && saved.container.as_deref() != Some(id)
        {
            return Err("recovery_container_changed".into());
        }
        Ok(())
    }
}

pub(super) fn valid_id(id: &str) -> Result<(), String> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("recovery_container_id_invalid".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "container_tests.rs"]
mod tests;
