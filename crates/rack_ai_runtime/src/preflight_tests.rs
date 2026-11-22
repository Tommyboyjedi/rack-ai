use super::{MemoryProbe, Preflight};
use crate::config::{Config, Driver, Profile};

fn host_only_profile(config: &Config) -> Profile {
    let mut profile = config.profiles[0].clone();
    profile.driver = Driver::Systemd;
    profile.resources.clear();
    profile.device_mib.clear();
    // Deterministically exceeds physical RAM without allocating memory.
    profile.host_mib = u64::MAX;
    profile
}

fn config() -> Config {
    serde_json::from_str(include_str!("../../../config/runtime/config.example.json")).unwrap()
}

#[test]
fn before_preemption_cleanup_checks_device_capacity_without_requiring_free_host_ram() {
    let config = config();
    let profile = host_only_profile(&config);
    assert_eq!(
        (MemoryProbe { config: &config }).memory(&profile, false),
        Ok(())
    );
}

#[test]
fn after_preemption_cleanup_still_rejects_insufficient_physical_host_ram() {
    let config = config();
    let profile = host_only_profile(&config);
    assert_eq!(
        (Preflight { config: &config }).empty(&profile),
        Err("insufficient_host_memory".into())
    );
}

#[test]
fn after_preemption_cleanup_accepts_sufficient_physical_host_ram() {
    let config = config();
    let mut profile = host_only_profile(&config);
    profile.host_mib = 0;
    assert_eq!((Preflight { config: &config }).empty(&profile), Ok(()));
}
