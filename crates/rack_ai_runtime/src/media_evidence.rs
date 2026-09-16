//! Reconcile media only from canonical ownership and OS evidence, never HTTP health.
use crate::{
    media::MediaAdapter,
    service::{Service, owns},
    types::*,
};
use rack_ai_media::{
    runtime::Runtime,
    types::{Service as MediaService, ServiceState},
};

pub struct MediaEvidence<'a> {
    pub service: &'a Service,
    pub runtime: &'a Runtime,
}
impl MediaEvidence<'_> {
    pub fn bound(&self, d: &Demand) -> Result<MediaService, String> {
        self.service.authority.read(|s| {
            let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
            if saved.generation != d.generation || !owns(s, saved) {
                return Err("media_recovery_claim_changed".into());
            }
            Ok(())
        })?;
        let p = d.process.as_ref().ok_or("media_recovery_process_unknown")?;
        if p.activation != d.generation || p.unit.as_ref() != Some(&self.runtime.config.unit) {
            return Err("media_recovery_process_binding_changed".into());
        }
        let state = self.runtime.store.read()?;
        let media = state.service;
        if media.state == ServiceState::Stopped
            && media.lease.is_none()
            && media.activation.is_empty()
        {
            if state
                .jobs
                .iter()
                .any(|j| !j.state.terminal() || j.cleanup_pending)
                || state
                    .sessions
                    .iter()
                    .any(|s| !s.stopped && !s.release_requested)
            {
                return Err("media_recovery_pending_work".into());
            }
            return Ok(media);
        }
        let handle = media.lease.as_ref().ok_or("media_recovery_lease_missing")?;
        if media.activation != d.generation
            || handle.owner != d.id
            || handle.generation != d.generation
        {
            return Err("media_recovery_activation_changed".into());
        }
        self.runtime.reservations.verify(handle)?;
        Ok(media)
    }
    pub fn stopped(&self, d: &Demand) -> Result<bool, String> {
        let media = self.bound(d)?;
        if media
            .restart
            .as_ref()
            .is_some_and(|r| r.phase != rack_ai_media::restart_state::RestartPhase::Completed)
        {
            return Ok(false);
        }
        if !self.runtime.systemd.gone()? {
            return Ok(false);
        }
        let mut process = d.process.clone().ok_or("media_recovery_process_unknown")?;
        if !crate::process::gone(&process)? {
            return Err("media_recovery_recorded_process_alive".into());
        }
        if let Some(generation) = media.generation {
            process.pid = generation.pid;
            process.start = generation.start_ticks.to_string();
            if !crate::process::gone(&process)? {
                return Err("media_recovery_generation_alive".into());
            }
        }
        if !(rack_ai_media::gpu::GpuProbe {
            config: &self.runtime.config,
        })
        .pids()?
        .is_empty()
        {
            return Err("media_recovery_gpu_not_clear".into());
        }
        Ok(true)
    }
    pub fn owned(&self, d: &Demand) -> Result<MediaService, String> {
        let media = self.bound(d)?;
        let observed = self.runtime.systemd.observe()?;
        if observed.pending_job || media.invocation.as_ref() != Some(&observed.invocation) {
            return Err("media_recovery_invocation_changed".into());
        }
        media
            .generation
            .as_ref()
            .ok_or("media_recovery_generation_unknown")?
            .verify(&observed)?;
        (rack_ai_media::gpu::GpuProbe {
            config: &self.runtime.config,
        })
        .owned(&observed)?;
        Ok(media)
    }
}
pub fn supported(d: &Demand) -> bool {
    d.profile.backend == crate::config::Backend::Comfyui
        && d.profile.driver != crate::config::Driver::Fixture
}
pub fn runtime(service: &Service, d: &Demand) -> Result<Runtime, String> {
    (MediaAdapter {
        config: &service.config,
    })
    .runtime(d)
}
pub fn transport(error: &str) -> bool {
    matches!(
        error,
        "backend transport uncertain" | "backend_transport_uncertain" | "backend read uncertain"
    ) || error.starts_with("backend HTTP 5")
}
