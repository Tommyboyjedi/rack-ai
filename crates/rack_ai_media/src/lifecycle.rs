use crate::{gpu::GpuProbe, runtime::Runtime, types::*};
use rack_ai_application::durable_file::atomic_write;
pub struct Lifecycle<'a> {
    pub runtime: &'a Runtime,
}
impl Lifecycle<'_> {
    pub fn tick(&self) -> Result<(), String> {
        let r = self.runtime;
        let state = r.store.read()?;
        let service = &state.service;
        match service.state {
            ServiceState::Stopped | ServiceState::Waiting => self.begin(&state),
            ServiceState::Reserving => {
                Err("interrupted reservation intent requires reconciliation".into())
            }
            ServiceState::Starting => crate::startup::StartupObservation { runtime: r }.observe(),
            ServiceState::Ready => self.ready(&state),
            ServiceState::Draining => crate::shutdown::Shutdown { runtime: r }.drain(service),
            ServiceState::Stopping => crate::shutdown::Shutdown { runtime: r }.stopping(service),
            ServiceState::RecoveryRequired => Ok(()),
        }
    }
    fn begin(&self, state: &MediaState) -> Result<(), String> {
        let r = self.runtime;
        let mode = if state
            .sessions
            .iter()
            .any(|s| !s.stopped && !s.release_requested)
        {
            Mode::Interactive
        } else if state.jobs.iter().any(|j| !j.state.terminal()) {
            Mode::Managed
        } else {
            return Ok(());
        };
        crate::activation::Activation { runtime: r }.begin(mode)
    }
    fn ready(&self, state: &MediaState) -> Result<(), String> {
        let r = self.runtime;
        let s = &state.service;
        self.verify(s)?;
        self.authority(s)?;
        let expired = state.sessions.iter().any(|x| {
            Some(&x.id) == s.session.as_ref() && now() > x.created_at + r.config.session_seconds
        });
        let finish = if s.mode == Mode::Interactive {
            !state
                .sessions
                .iter()
                .any(|x| Some(&x.id) == s.session.as_ref() && !x.release_requested)
                || expired
        } else {
            !state
                .jobs
                .iter()
                .any(|j| !j.state.terminal() || j.cleanup_pending)
                && now() >= s.last_busy + r.config.idle_seconds
        };
        if finish {
            self.transition(ServiceState::Draining)?;
        }
        Ok(())
    }

    pub fn verify(&self, service: &Service) -> Result<(), String> {
        let r = self.runtime;
        r.reservations
            .verify(service.lease.as_ref().ok_or("missing media reservation")?)?;
        let observed = r.systemd.observe()?;
        if service.invocation.as_ref() != Some(&observed.invocation) {
            return Err("backend invocation changed".into());
        }
        GpuProbe { config: &r.config }.owned(&observed)?;
        let gate = r.backend.gate()?;
        if gate.activation != service.activation
            || gate.invocation != observed.invocation
            || gate.pid != observed.pid
            || gate.protocol != "rack-gate/v1"
        {
            return Err("gate/process identity mismatch".into());
        }
        Ok(())
    }
    pub fn authority(&self, service: &Service) -> Result<(), String> {
        atomic_write(
            &self.runtime.config.authority_file,
            &serde_json::json!({
                "activation":service.activation,"mode":service.mode,"expires":now()+crate::limits::AUTHORITY_SECONDS
            })
            .to_string(),
        )
    }
    pub fn transition(&self, next: ServiceState) -> Result<(), String> {
        self.runtime.store.update(|state| {
            state.service.state = next;
            state.service.since = now();
            Ok(())
        })
    }
}
