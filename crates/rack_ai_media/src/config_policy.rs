//! Independent listener, deployment-path and authenticated-source configuration policies.
use crate::config::Config;
use std::{ffi::OsStr, path::Path};
pub fn validate(config: &Config) -> Result<(), String> {
    NetworkPolicy { config }.validate()?;
    LocalPolicy { config }.validate()?;
    SourcePolicy { config }.validate()
}
struct NetworkPolicy<'a> {
    config: &'a Config,
}
impl NetworkPolicy<'_> {
    fn validate(&self) -> Result<(), String> {
        if !self.config.listen.ip().is_loopback()
            || !self.config.native_listen.ip().is_loopback()
            || self.config.listen == self.config.native_listen
        {
            return Err("distinct loopback listeners required".into());
        }
        let backend = reqwest::Url::parse(&self.config.backend).map_err(|e| e.to_string())?;
        if backend.scheme() != "http"
            || backend.host_str() != Some("127.0.0.1")
            || backend.path() != "/"
            || backend.query().is_some()
            || !backend.username().is_empty()
        {
            return Err("raw ComfyUI must use loopback".into());
        }
        if self.config.public_origin == self.config.native_origin {
            return Err("native UI requires its own origin".into());
        }
        for origin in [&self.config.public_origin, &self.config.native_origin] {
            let url = reqwest::Url::parse(origin).map_err(|e| e.to_string())?;
            if url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
                || url.password().is_some()
                || !url.username().is_empty()
                || !(url.scheme() == "https"
                    || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1")))
            {
                return Err("private HTTPS origins (or loopback test origins) required".into());
            }
        }
        Ok(())
    }
}
struct LocalPolicy<'a> {
    config: &'a Config,
}
impl LocalPolicy<'_> {
    fn validate(&self) -> Result<(), String> {
        let input_root = self.config.input_root();
        for root in [
            &self.config.state_root,
            &self.config.resource_root,
            &input_root,
            &self.config.output_root,
            &self.config.authority_file,
            &self.config.control_secret_file,
            &self.config.runtime.python,
            &self.config.runtime.script,
            &self.config.runtime.directory,
        ] {
            if !root.is_absolute() {
                return Err("absolute administrator paths required".into());
            }
        }
        if self
            .config
            .browser_auth_file
            .as_ref()
            .is_some_and(|p| !p.is_absolute())
        {
            return Err("absolute browser authentication path required".into());
        }
        self.cleanup_roots(&input_root)?;
        if !self.config.unit.starts_with("rack-ai-comfyui-")
            || !self.config.unit.ends_with(".service")
            || !self
                .config
                .unit
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c))
        {
            return Err("invalid owned candidate unit".into());
        }
        if [
            self.config.start_timeout,
            self.config.drain_timeout,
            self.config.stop_timeout,
            self.config.idle_seconds,
            self.config.reservation_idle_seconds,
            self.config.session_seconds,
        ]
        .iter()
        .any(|n| *n == 0 || *n > 86400)
        {
            return Err("invalid lifecycle deadline".into());
        }
        Ok(())
    }

    fn cleanup_roots(&self, input_root: &Path) -> Result<(), String> {
        if input_root == self.config.output_root {
            return Err("distinct ComfyUI cleanup roots required".into());
        }
        if input_root.file_name() != Some(OsStr::new("input"))
            || self.config.output_root.file_name() != Some(OsStr::new("output"))
            || input_root.parent() != self.config.output_root.parent()
        {
            return Err("ComfyUI cleanup roots must be sibling input/output directories".into());
        }
        for root in [input_root, self.config.output_root.as_path()] {
            if root.parent().and_then(Path::parent).is_none() {
                return Err("ComfyUI cleanup roots are too broad".into());
            }
        }
        Ok(())
    }
}
struct SourcePolicy<'a> {
    config: &'a Config,
}
impl SourcePolicy<'_> {
    fn validate(&self) -> Result<(), String> {
        if self.config.principals.is_empty()
            || self.config.principals.iter().any(|p| {
                p.id.is_empty()
                    || p.token_sha256.len() != 64
                    || !p.token_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Err("invalid principal credentials".into());
        }
        let ids: std::collections::BTreeSet<_> =
            self.config.principals.iter().map(|p| &p.id).collect();
        let hashes: std::collections::BTreeSet<_> = self
            .config
            .principals
            .iter()
            .map(|p| p.token_sha256.to_ascii_lowercase())
            .collect();
        if hashes.len() != self.config.principals.len() || ids.len() != self.config.principals.len()
        {
            return Err("duplicate principal".into());
        }
        Ok(())
    }
}
