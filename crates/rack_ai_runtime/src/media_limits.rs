use crate::{config::Profile, types::Demand};
use rack_ai_media::{command::run, config::Config, unit_definition::UnitDefinition};
/// Validate the administrator-owned permanent unit without changing its configuration.
pub fn verify(d: &Demand) -> Result<(), String> {
    let c = configuration(d)?;
    UnitDefinition { config: &c }.verify()?;
    let raw = run(
        "systemctl",
        &[
            "--user",
            "show",
            &c.unit,
            "-p",
            "MemoryMax",
            "-p",
            "MemorySwapMax",
            "-p",
            "CPUQuotaPerSecUSec",
        ],
    )?;
    limits(&d.profile, &raw)
}
fn limits(profile: &Profile, raw: &str) -> Result<(), String> {
    let values: std::collections::BTreeMap<_, _> =
        raw.lines().filter_map(|l| l.split_once('=')).collect();
    let memory = values
        .get("MemoryMax")
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or("media_memory_limit_unknown")?;
    let quota = values
        .get("CPUQuotaPerSecUSec")
        .ok_or("media_cpu_limit_unknown")?;
    let micros = [("ms", 1000.0), ("us", 1.0), ("s", 1000000.0)]
        .iter()
        .find_map(|(suffix, scale)| {
            quota
                .strip_suffix(suffix)
                .and_then(|v| v.parse::<f64>().ok())
                .map(|v| v * scale)
        })
        .ok_or("media_cpu_limit_unknown")?;
    if memory == 0
        || memory > profile.host_mib.saturating_mul(1024 * 1024)
        || values.get("MemorySwapMax") != Some(&"0")
        || !micros.is_finite()
        || micros <= 0.0
        || micros > f64::from(profile.cpu_percent) * 10000.0
    {
        return Err("media_unit_exceeds_reserved_limits".into());
    }
    Ok(())
}

pub fn configuration(d: &Demand) -> Result<Config, String> {
    let path = d
        .profile
        .media_config
        .clone()
        .ok_or("media_configuration_required")?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if Some(&configuration_digest(&bytes, d.profile.native_media())?)
        != d.profile.media_config_sha256.as_ref()
    {
        return Err("media_configuration_hash_mismatch".into());
    }
    let c = Config::load(path)?;
    let actual = std::fs::canonicalize(&c.runtime.python).map_err(|e| e.to_string())?;
    let pinned = std::fs::canonicalize(&d.profile.executable).map_err(|e| e.to_string())?;
    if actual != pinned {
        return Err("media_executable_binding_mismatch".into());
    }
    Ok(c)
}

/// Native authority pins service/security configuration, not the optional image recipe.
pub fn configuration_digest(bytes: &[u8], native: bool) -> Result<String, String> {
    if !native {
        return Ok(crate::types::digest(bytes));
    }
    let mut value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    value
        .as_object_mut()
        .ok_or("invalid_media_configuration")?
        .remove("profile");
    Ok(crate::types::digest(
        &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::configuration_digest;
    #[test]
    fn native_binding_excludes_only_image_recipe() {
        let original =
            br#"{"unit":"owned.service","profile":{"checkpoint":"one","checkpoint_sha256":"a"}}"#;
        let recipe=br#"{"unit":"owned.service","profile":{"checkpoint":"missing","checkpoint_sha256":"b"}}"#;
        let service =
            br#"{"unit":"foreign.service","profile":{"checkpoint":"one","checkpoint_sha256":"a"}}"#;
        assert_eq!(
            configuration_digest(original, true).unwrap(),
            configuration_digest(recipe, true).unwrap()
        );
        assert_ne!(
            configuration_digest(original, false).unwrap(),
            configuration_digest(recipe, false).unwrap()
        );
        assert_ne!(
            configuration_digest(original, true).unwrap(),
            configuration_digest(service, true).unwrap()
        );
    }
}
