//! Reset a configured failed media unit only under the canonical authority lock.
use crate::{config::Profile, service::Service};
use ResourceStatus::*;
use rack_ai_media::resource_command::ResourceStatus;
use rack_ai_media::{
    config::Config,
    gpu::GpuProbe,
    runtime::Runtime,
    types::{Mode, ServiceState},
    unit_definition::UnitDefinition,
};

pub fn reset(service: &Service, input: (&Profile, bool)) -> Result<ResourceStatus, String> {
    let (profile, apply) = input;
    // Probe failures are per-resource blockers, not permission to reset.
    inspect(service, (profile, apply)).or_else(|_| Ok(BlockedPhysicalVerification))
}
fn inspect(service: &Service, input: (&Profile, bool)) -> Result<ResourceStatus, String> {
    let (profile, apply) = input;
    let path = profile
        .media_config
        .as_ref()
        .ok_or("missing media configuration")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if Some(crate::media_limits::configuration_digest(
        &bytes,
        profile.native_media(),
    )?) != profile.media_config_sha256
    {
        return Err("media configuration binding changed".into());
    }
    let config = Config::load(path.clone())?;
    if config.resource_root != service.config.authority_root
        || profile.resources.len() != 1
        || service
            .config
            .devices
            .get(&profile.resources[0])
            .is_none_or(|d| d.uuid != config.media_uuid)
    {
        return Err("media authority binding changed".into());
    }
    let runtime = Runtime::new(config)?;
    let state = runtime.store.read()?;
    let orphan = rack_ai_media::orphan_recovery::empty(&state);
    if (!orphan && state.service.state != ServiceState::Stopped)
        || state.service.mode != Mode::Closed
        || state.sessions.iter().any(|s| !s.stopped)
        || state
            .jobs
            .iter()
            .any(|j| !j.state.terminal() || j.cleanup_pending)
    {
        return Ok(MediaSessionPreserved);
    }
    UnitDefinition {
        config: &runtime.config,
    }
    .verify()?;
    let observed = runtime.systemd.observe()?;
    if !runtime
        .reservations
        .legacy_blocked(&profile.resources)?
        .is_empty()
    {
        return Ok(BlockedLiveOrUnprovenProcess);
    }
    if !matches!(observed.active.as_str(), "inactive" | "failed")
        || observed.pid != 0
        || observed.pending_job
        || !runtime.systemd.gone()?
        || !(GpuProbe {
            config: &runtime.config,
        })
        .pids()?
        .is_empty()
    {
        return Ok(BlockedLiveOrUnprovenProcess);
    }
    if observed.active == "inactive" && observed.invocation.is_empty() && !orphan {
        return Ok(Available);
    }
    if !apply {
        return Ok(ResetAvailable);
    }
    if observed.active == "failed" || !observed.invocation.is_empty() {
        rack_ai_media::command::run(
            "systemctl",
            &["--user", "reset-failed", &runtime.config.unit],
        )?;
    }
    if orphan {
        rack_ai_media::orphan_recovery::clear(&runtime)?;
    }
    let after = runtime.systemd.observe()?;
    if after.active != "inactive"
        || after.pid != 0
        || !after.invocation.is_empty()
        || after.pending_job
    {
        return Err("reset did not clear failed identity".into());
    }
    Ok(ResetComplete)
}
