use crate::{command::run, config::Config};
pub struct UnitDefinition<'a> {
    pub config: &'a Config,
}
impl UnitDefinition<'_> {
    pub fn verify(&self) -> Result<(), String> {
        let config = self.config;
        let raw = run(
            "systemctl",
            &[
                "--user",
                "show",
                &config.unit,
                "-p",
                "Id",
                "-p",
                "ExecStart",
                "-p",
                "WorkingDirectory",
                "-p",
                "Environment",
                "-p",
                "Type",
                "-p",
                "Restart",
                "-p",
                "KillMode",
            ],
        )?;
        let values: std::collections::BTreeMap<_, _> =
            raw.lines().filter_map(|l| l.split_once('=')).collect();
        if values.get("Id") != Some(&config.unit.as_str())
            || values.get("Type") != Some(&"simple")
            || values.get("Restart") != Some(&"no")
            || values.get("KillMode") != Some(&"control-group")
            || values.get("WorkingDirectory").copied() != config.runtime.directory.to_str()
        {
            return Err("unexpected managed unit definition".into());
        }
        let command = values
            .get("ExecStart")
            .ok_or("missing unit runtime command")?;
        let expected = format!(
            "{{ path={} ; argv[]={} {} ",
            config.runtime.python.display(),
            config.runtime.python.display(),
            config.runtime.script.display()
        );
        if !command.starts_with(&expected) || command.matches("{ path=").count() != 1 {
            return Err("unexpected configured ComfyUI runtime".into());
        }
        let environment = values
            .get("Environment")
            .ok_or("missing unit environment")?;
        for (key, value) in [
            ("CUDA_VISIBLE_DEVICES", config.media_uuid.as_str()),
            (
                "RACK_MEDIA_AUTHORITY_FILE",
                config
                    .authority_file
                    .to_str()
                    .ok_or("invalid authority path")?,
            ),
            (
                "RACK_MEDIA_CONTROL_SECRET_FILE",
                config
                    .control_secret_file
                    .to_str()
                    .ok_or("invalid control path")?,
            ),
        ] {
            let expected = format!("{key}={value}");
            if environment
                .split_whitespace()
                .filter(|v| v.starts_with(&format!("{key}=")))
                .collect::<Vec<_>>()
                != vec![expected.as_str()]
            {
                return Err(format!("unexpected unit {key} binding"));
            }
        }
        Ok(())
    }
}
