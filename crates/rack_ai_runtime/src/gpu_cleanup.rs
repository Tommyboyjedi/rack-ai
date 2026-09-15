//! GPU allocations can outlive process exit briefly. Keep claims until they disappear.
use crate::{
    config::{Config, Profile},
    preflight::Preflight,
};
use std::time::{Duration, Instant};
const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn wait(config: &Config, profile: &Profile) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(profile.stop_seconds);
    loop {
        match (Preflight { config }).reclaimed(profile) {
            Ok(()) => return Ok(()),
            Err(error) if error == "gpu_cleanup_uncertain" => {
                if Instant::now() >= deadline {
                    return Err("gpu_cleanup_deadline".into());
                }
            }
            Err(error) => return Err(error),
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}
