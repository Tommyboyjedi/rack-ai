use crate::{backend_generation::BackendGeneration, config::Config, systemd::Observation};
use serde::Deserialize;
use std::{fs, path::PathBuf};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePaths {
    pub python: PathBuf,
    pub script: PathBuf,
    pub directory: PathBuf,
}
impl Default for RuntimePaths {
    fn default() -> Self {
        Self {
            python: "/srv/comfyui/venv/bin/python".into(),
            script: "/srv/comfyui/ComfyUI/main.py".into(),
            directory: "/srv/comfyui/ComfyUI".into(),
        }
    }
}
pub struct ProcessIdentity<'a> {
    pub config: &'a Config,
}
impl ProcessIdentity<'_> {
    pub fn verify(&self, observed: &Observation) -> Result<(), String> {
        let generation = BackendGeneration::capture(0, observed)?;
        let root = PathBuf::from(format!("/proc/{}", observed.pid));
        let expected = &self.config.runtime;
        let args = fs::read(root.join("cmdline")).map_err(|_| "runtime command unavailable")?;
        let args: Vec<_> = args.split(|b| *b == 0).collect();
        use std::os::unix::ffi::OsStrExt;
        if args.first().copied() != Some(expected.python.as_os_str().as_bytes())
            || args.get(1).copied() != Some(expected.script.as_os_str().as_bytes())
            || fs::read_link(root.join("exe")).map_err(|_| "runtime executable unavailable")?
                != expected
                    .python
                    .canonicalize()
                    .map_err(|_| "expected Python unavailable")?
            || fs::read_link(root.join("cwd")).map_err(|_| "runtime directory unavailable")?
                != expected
                    .directory
                    .canonicalize()
                    .map_err(|_| "expected runtime directory unavailable")?
        {
            return Err("unexpected managed ComfyUI runtime".into());
        }
        let env = fs::read(root.join("environ")).map_err(|_| "runtime environment unavailable")?;
        for (key, value) in [
            ("CUDA_VISIBLE_DEVICES", self.config.media_uuid.as_str()),
            (
                "RACK_MEDIA_AUTHORITY_FILE",
                self.config
                    .authority_file
                    .to_str()
                    .ok_or("invalid authority path")?,
            ),
            (
                "RACK_MEDIA_CONTROL_SECRET_FILE",
                self.config
                    .control_secret_file
                    .to_str()
                    .ok_or("invalid control path")?,
            ),
        ] {
            let expected = format!("{key}={value}");
            if env
                .split(|b| *b == 0)
                .filter(|entry| entry.starts_with(format!("{key}=").as_bytes()))
                .collect::<Vec<_>>()
                != vec![expected.as_bytes()]
            {
                return Err(format!("unexpected managed runtime {key} binding"));
            }
        }
        generation.verify(observed)
    }
}
