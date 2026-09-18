//! Cleanup under the media operation lock, using generation evidence rather than health.
use crate::types::{Demand, Process};
use rack_ai_media::{runtime::Runtime, types::*};

pub struct MediaCleanup<'a> {
    pub runtime: &'a Runtime,
}
impl MediaCleanup<'_> {
    pub fn run(&self, d: &Demand) -> Result<Option<Process>, String> {
        let r = self.runtime;
        let state = r.store.read()?;
        let service = &state.service;
        binding(&state, d)?;
        let process = MediaIdentity { runtime: r }.identity(d, service)?;
        // Revoke the native gate and all same-generation recreation intents
        // before requesting a stop. No new activation is ever issued here.
        let mut closed = service.clone();
        closed.mode = Mode::Closed;
        rack_ai_media::lifecycle::Lifecycle { runtime: r }.authority(&closed)?;
        r.store.update(|s| {
            binding(s, d)?;
            s.service.mode = Mode::Closed;
            s.service.state = ServiceState::RecoveryRequired;
            s.service.restart = None;
            for session in s.sessions.iter_mut().filter(|session| !session.stopped) {
                session.release_requested = true;
            }
            Ok(())
        })?;
        if !r.systemd.gone()? {
            let mut expected = closed.clone();
            expected.invocation = process
                .as_ref()
                .and_then(|p| p.invocation.clone())
                .or_else(|| service.invocation.clone());
            r.systemd.stop(&expected)?;
        }
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(d.profile.stop_seconds);
        loop {
            if r.systemd.gone()?
                && process
                    .as_ref()
                    .map(crate::process::gone)
                    .transpose()?
                    .unwrap_or(true)
                && (rack_ai_media::gpu::GpuProbe { config: &r.config })
                    .pids()?
                    .is_empty()
            {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err("media_recovery_cleanup_deadline".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        r.store.update(|s| {
            binding(s, d)?;
            for session in s.sessions.iter_mut().filter(|session| !session.stopped) {
                session.stopped = true;
                session.release_requested = true;
                session.terminal_reason = Some("reservation_recovery_cleanup".into());
            }
            for job in s
                .jobs
                .iter_mut()
                .filter(|job| !job.state.terminal() || job.cleanup_pending)
            {
                job.cleanup_pending = false;
                if !job.state.terminal() {
                    job.state = if job.dispatched_at.is_some() {
                        JobState::Interrupted
                    } else {
                        JobState::Cancelled
                    };
                    job.error.get_or_insert(
                        "recovery cleanup; historical outcome unknown; no replay".into(),
                    );
                }
            }
            s.service = Service::default();
            Ok(())
        })?;
        Ok(process)
    }
}

struct MediaIdentity<'a> {
    runtime: &'a Runtime,
}
impl MediaIdentity<'_> {
    fn identity(&self, d: &Demand, service: &Service) -> Result<Option<Process>, String> {
        let r = self.runtime;
        let observed = r.systemd.observe()?;
        if r.systemd.gone()? {
            if let Some(p) = &d.process
                && (p.activation != d.generation
                    || p.unit.as_ref() != Some(&r.config.unit)
                    || !crate::process::gone(p)?)
            {
                return Err("media_recovery_recorded_process_changed".into());
            }
            if let Some(generation) = &service.generation {
                crate::recovery_media_group::require_main_gone(generation)?;
            }
            return Ok(d.process.clone());
        }
        if observed.pid == 0 {
            return (crate::recovery_media_group::GroupCleanup { runtime: r })
                .process(d, &observed);
        }
        if observed.pending_job {
            return Err("media_recovery_pending_systemd_job".into());
        }
        if let Some(p) = &d.process {
            if p.activation != d.generation || p.unit.as_ref() != Some(&r.config.unit) {
                return Err("media_recovery_process_binding_changed".into());
            }
            if p.pid == observed.pid {
                let current = crate::process::capture(observed.pid, &d.generation)?;
                if p.boot != current.boot || p.start != current.start {
                    return Err("media_recovery_recorded_generation_changed".into());
                }
            } else if !crate::process::gone(p)? {
                return Err("media_recovery_previous_process_still_alive".into());
            }
        }
        if service
            .invocation
            .as_ref()
            .is_some_and(|id| id != &observed.invocation)
        {
            return Err("media_recovery_invocation_changed".into());
        }
        if let Some(generation) = &service.generation {
            generation.verify(&observed)?;
        } else if let Some(p) = &d.process {
            if p.activation != d.generation
                || p.pid != observed.pid
                || p.unit.as_ref() != Some(&r.config.unit)
                || p.invocation.as_ref() != Some(&observed.invocation)
                || crate::process::gone(p)?
            {
                return Err("media_recovery_process_binding_changed".into());
            }
        } else {
            let gate = r.backend.gate()?;
            if gate.activation != d.generation
                || gate.invocation != observed.invocation
                || gate.pid != observed.pid
                || gate.protocol != "rack-gate/v1"
            {
                return Err("media_recovery_unattributed_process".into());
            }
        }
        rack_ai_media::process_identity::ProcessIdentity { config: &r.config }.verify(&observed)?;
        rack_ai_media::gpu::GpuProbe { config: &r.config }.owned(&observed)?;
        let mut process = crate::process::capture(observed.pid, &d.generation)?;
        process.unit = Some(r.config.unit.clone());
        process.invocation = Some(observed.invocation);
        Ok(Some(process))
    }
}

fn binding(state: &MediaState, d: &Demand) -> Result<(), String> {
    let media = &state.service;
    if !media.activation.is_empty() && media.activation != d.generation {
        return Err("media_recovery_foreign_activation".into());
    }
    match &media.lease {
        Some(handle) if handle.owner == d.id && handle.generation == d.generation => {}
        None if media.activation.is_empty()
            && media.generation.is_none()
            && media.invocation.is_none()
            && media.restart.is_none() => {}
        _ => return Err("media_recovery_lease_binding_changed".into()),
    }
    if state
        .sessions
        .iter()
        .any(|s| !s.stopped && (s.owner != d.owner || s.request.idempotency_key != d.id))
    {
        return Err("media_recovery_foreign_session".into());
    }
    if state.jobs.iter().any(|j| {
        (!j.state.terminal() || j.cleanup_pending)
            && (j.owner != d.owner
                || j.activation.as_ref().is_some_and(|a| a != &d.generation)
                || j.request
                    .reservation
                    .as_ref()
                    .is_none_or(|r| r.id != d.id || r.generation != d.generation))
    }) {
        return Err("media_recovery_foreign_job".into());
    }
    Ok(())
}
