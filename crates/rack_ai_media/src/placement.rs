use crate::{command::run, config::Config};
pub struct PlacementProbe<'a> {
    pub config: &'a Config,
}
impl PlacementProbe<'_> {
    pub fn verify(&self) -> Result<(), String> {
        let inventory = run(
            "nvidia-smi",
            &["--query-gpu=uuid,name", "--format=csv,noheader,nounits"],
        )?;
        let rows: Vec<_> = inventory
            .lines()
            .filter_map(|l| l.split_once(','))
            .map(|(id, name)| (id.trim(), name.trim()))
            .collect();
        let ids: std::collections::BTreeSet<_> = rows.iter().map(|(id, _)| *id).collect();
        if ids.len() != rows.len()
            || self.config.media_uuid.len() != 40
            || rows
                .iter()
                .filter(|(id, name)| *id == self.config.media_uuid && name.contains("4080 SUPER"))
                .count()
                != 1
        {
            return Err("media UUID does not identify one 4080 SUPER".into());
        }
        let mut protected = std::collections::BTreeSet::new();
        for worker in &self.config.protected {
            if !ids.contains(worker.uuid.as_str())
                || worker.uuid == self.config.media_uuid
                || !protected.insert(&worker.uuid)
            {
                return Err("missing/duplicate/overlapping protected GPU UUID".into());
            }
            let raw = run(
                "docker",
                &[
                    "inspect",
                    &worker.container,
                    "--format",
                    "{{json .HostConfig.DeviceRequests}}",
                ],
            )?;
            let devices: serde_json::Value =
                serde_json::from_str(&raw).map_err(|_| "invalid protected placement")?;
            let requests = devices
                .as_array()
                .ok_or("missing protected device requests")?;
            if requests.len() != 1
                || requests[0].get("DeviceIDs") != Some(&serde_json::json!([worker.uuid]))
            {
                return Err(
                    "protected live inference binding differs from administrator mapping".into(),
                );
            }
        }
        if protected.len() != 2 {
            return Err("both development GPUs must be protected".into());
        }
        Ok(())
    }
}
