use crate::{config::*, types::*};
use std::collections::BTreeSet;
fn unique<T: Ord>(items: impl Iterator<Item = T>, count: usize) -> bool {
    items.collect::<BTreeSet<_>>().len() == count
}
pub fn validate(c: &Config) -> Result<(), String> {
    c.limits.validate()?;
    c.interaction_diagnostics.validate()?;
    let address: std::net::SocketAddr = c.listen.parse().map_err(|_| "invalid listen address")?;
    if c.schema != VERSION
        || !address.ip().is_loopback()
        || address.port() == 0
        || !c.authority_root.is_absolute()
        || c.idle_timeout_seconds == 0
        || c.idle_timeout_seconds > 86400
        || c.max_ttl_seconds == 0
        || c.max_ttl_seconds > 86400
        || c.host_capacity_mib == 0
    {
        return Err("invalid server configuration".into());
    }
    if !unique(c.devices.values().map(|d| &d.uuid), c.devices.len())
        || !unique(c.profiles.iter().map(|p| &p.tag), c.profiles.len())
        || !unique(c.sources.iter().map(|s| &s.source), c.sources.len())
        || !unique(c.sources.iter().map(|s| &s.token_sha256), c.sources.len())
    {
        return Err("duplicate physical device, tag or principal".into());
    }
    for (id, device) in &c.devices {
        if !valid_id(id)
            || device.capacity_mib == 0
            || (!c.fixture_mode && (!device.uuid.starts_with("GPU-") || device.uuid.len() != 40))
        {
            return Err("invalid device".into());
        }
    }
    for (index, p) in c.profiles.iter().enumerate() {
        if reqwest::Url::parse(&p.endpoint)
            .ok()
            .and_then(|url| url.port())
            == Some(address.port())
        {
            return Err("gateway cannot reuse a backend port".into());
        }
        if c.profiles.iter().skip(index + 1).any(|other| {
            p.endpoint.trim_end_matches('/') == other.endpoint.trim_end_matches('/')
                && !p.resources.iter().any(|r| other.resources.contains(r))
        }) {
            return Err("one endpoint cannot represent disjoint resources".into());
        }
    }
    for p in &c.profiles {
        ProfileValidation { config: c }.validate(p)?;
    }
    if c.workspace.is_some()
        && (c.limits.max_dispatch_workers < 2 || c.limits.max_pending_per_reservation < 2)
    {
        return Err("workspace requires a dispatch slot for nested model calls".into());
    }
    for s in &c.sources {
        if !valid_id(&s.source)
            || s.source == "*"
            || s.token_sha256.len() != 64
            || !s.token_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalid principal credentials".into());
        }
    }
    Ok(())
}
const IMAGE_REQUEST_ENVELOPE_BYTES: usize = 16 * 1024;
const DATA_IMAGE_PREFIX_BYTES: usize = 32;

fn image_request_bytes(p: &Profile) -> usize {
    let encoded = usize::try_from(p.max_image_bytes)
        .ok()
        .and_then(|bytes| bytes.checked_add(2))
        .map(|bytes| (bytes / 3) * 4)
        .unwrap_or(usize::MAX);
    usize::try_from(p.max_images_per_request)
        .ok()
        .and_then(|count| count.checked_mul(encoded.saturating_add(DATA_IMAGE_PREFIX_BYTES)))
        .and_then(|bytes| bytes.checked_add(IMAGE_REQUEST_ENVELOPE_BYTES))
        .unwrap_or(usize::MAX)
}

struct ProfileValidation<'a> {
    config: &'a Config,
}
impl ProfileValidation<'_> {
    fn validate(&self, p: &Profile) -> Result<(), String> {
        let c = self.config;
        crate::speech::profile(p)?;
        crate::launch_config::validate(p)?;
        crate::input_preflight::validate_profile(p)?;
        if p.backend == Backend::Chatterbox
            && !c.fixture_mode
            && c.devices
                .get("gpu-2060")
                .is_none_or(|d| d.uuid != "GPU-357ef569-8fac-7c7d-ee1c-51677efb174f")
        {
            return Err("chatterbox_exact_uuid_required".into());
        }
        let url = reqwest::Url::parse(&p.endpoint).map_err(|_| "invalid endpoint")?;
        if !valid_id(&p.tag)
            || p.version.is_empty()
            || (!p.native_media() && p.model.is_empty())
            || p.resources.is_empty()
            || !unique(p.resources.iter(), p.resources.len())
            || p.resources.iter().any(|r| !c.devices.contains_key(r))
            || p.device_mib.keys().collect::<BTreeSet<_>>() != p.resources.iter().collect()
            || p.device_mib
                .iter()
                .any(|(r, n)| *n == 0 || *n > c.devices[r].capacity_mib)
            || p.host_mib == 0
            || p.host_mib > c.host_capacity_mib
            || p.cpu_percent == 0
            || p.artifact_verify_seconds == 0
            || p.artifact_verify_seconds > 3600
            || p.context_tokens == 0
            || p.max_input_tokens == 0
            || p.max_output_tokens == 0
            || p.max_input_tokens > p.context_tokens
            || p.max_output_tokens > p.context_tokens
            || u64::from(p.max_input_tokens) + u64::from(p.max_output_tokens)
                > u64::from(p.context_tokens)
            || p.capabilities.is_empty()
            || (p.qualified && p.evidence.is_empty())
            || (p.driver == Driver::Fixture && !c.fixture_mode)
            || !p.executable.is_absolute()
            || p.executable_sha256.len() != 64
            || ![Some("127.0.0.1"), Some("[::1]")].contains(&url.host_str())
            || url.scheme() != "http"
            || url.port().is_none_or(|port| port == 0)
            || url.fragment().is_some()
            || url.path() != "/"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || [
                p.startup_seconds,
                p.drain_seconds,
                p.stop_seconds,
                p.inference_seconds,
            ]
            .iter()
            .any(|n| *n == 0 || *n > 3600)
        {
            return Err(format!("invalid runtime profile: {}", p.tag));
        }
        if (p.max_images_per_request > 0
            || p.max_image_bytes > 0
            || p.max_image_pixels > 0
            || p.image_mime_types.is_some())
            && (!p
                .capabilities
                .contains(&rack_ai_application::GenericCapability::Visual)
                || p.max_images_per_request == 0
                || p.max_images_per_request > 4
                || p.max_image_bytes == 0
                || p.max_image_bytes > 768 * 1024
                || p.max_image_pixels == 0
                || p.max_image_pixels > 16 * 1024 * 1024
                || p.image_mime_types.as_ref().is_some_and(|formats| {
                    formats.is_empty()
                        || formats.len() > crate::config::ImageMimeType::ALL.len()
                        || !unique(formats.iter(), formats.len())
                })
                || image_request_bytes(p) > crate::api::MAX_HTTP_BODY_BYTES)
        {
            return Err(format!("invalid runtime profile: {}", p.tag));
        }
        if p.driver != Driver::Fixture && p.backend != Backend::Comfyui {
            crate::network::private_binding(p)?;
        }
        if p.driver == Driver::Docker
            && (p.backend != Backend::Vllm
                || p.container_image
                    .as_ref()
                    .is_none_or(|i| !i.starts_with("sha256:") || i.len() != 71)
                || p.container_mounts.iter().any(|(source, target)| {
                    !source.is_absolute()
                        || !std::path::Path::new(target).is_absolute()
                        || target == "/"
                        || target.contains(',')
                        || source.to_string_lossy().contains(',')
                }))
        {
            return Err("invalid pinned container profile".into());
        }
        if p.backend == Backend::Comfyui
            && p.driver != Driver::Fixture
            && (p.media_config.is_none()
                || p.media_config_sha256
                    .as_ref()
                    .is_none_or(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit())))
        {
            return Err("comfyui requires pinned media configuration".into());
        }
        if matches!(p.backend, Backend::LlamaCpp | Backend::Strata)
            && (p.artifact.is_none() || p.artifact_sha256.as_ref().is_none_or(|h| h.len() != 64))
        {
            return Err("llama_cpp requires a pinned artifact".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod image_mime_tests {
    use super::*;

    fn config() -> Config {
        let mut config: Config =
            serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                .unwrap();
        config.profiles.retain(|profile| profile.tag == "tiny-uncd");
        config
    }

    #[test]
    fn omitted_and_null_image_mimes_preserve_legacy_profile_bytes_and_hash() {
        let profile = config().profiles.remove(0);
        let original = serde_json::to_vec(&profile).unwrap();
        let mut wire = serde_json::to_value(&profile).unwrap();
        assert!(wire.get("image_mime_types").is_none());
        assert_eq!(profile.effective_image_mime_types(), ImageMimeType::ALL);
        wire["image_mime_types"] = serde_json::Value::Null;
        let roundtrip: Profile = serde_json::from_value(wire).unwrap();
        let encoded = serde_json::to_vec(&roundtrip).unwrap();
        assert_eq!(encoded, original);
        assert_eq!(digest(&encoded), digest(&original));
    }

    #[test]
    fn image_mime_allowlist_requires_enabled_images_and_unique_supported_formats() {
        let mut config = config();
        assert!(validate(&config).is_ok());
        config.profiles[0].image_mime_types = Some(vec![ImageMimeType::Png]);
        assert!(validate(&config).is_err());
        let profile = &mut config.profiles[0];
        profile
            .capabilities
            .push(rack_ai_application::GenericCapability::Visual);
        profile.max_images_per_request = 2;
        profile.max_image_bytes = 262144;
        profile.max_image_pixels = 4194304;
        assert!(validate(&config).is_ok());
        for formats in [vec![], vec![ImageMimeType::Png, ImageMimeType::Png]] {
            config.profiles[0].image_mime_types = Some(formats);
            assert!(validate(&config).is_err());
        }
        config.profiles[0].image_mime_types = Some(vec![ImageMimeType::Jpeg, ImageMimeType::Png]);
        assert!(validate(&config).is_ok());
        let mut wire = serde_json::to_value(&config.profiles[0]).unwrap();
        assert_eq!(
            wire["image_mime_types"],
            serde_json::json!(["image/jpeg", "image/png"])
        );
        wire["image_mime_types"] = serde_json::json!(["image/avif"]);
        assert!(serde_json::from_value::<Profile>(wire).is_err());
    }
}
