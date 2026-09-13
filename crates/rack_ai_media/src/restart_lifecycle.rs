use crate::{gpu::GpuProbe, lifecycle::Lifecycle, restart_state::*, runtime::Runtime, types::*};
pub struct InteractiveRestart<'a> {
    pub runtime: &'a Runtime,
}
impl InteractiveRestart<'_> {
    pub fn tick(&self, state: &MediaState) -> Result<(), String> {
        let r = self.runtime;
        let s = &state.service;
        if s.mode != Mode::Interactive || s.session.is_none() {
            return Err("restart intent outside interactive activation".into());
        }
        r.reservations
            .verify(s.lease.as_ref().ok_or("restart lost reservation")?)?;
        let intent = s
            .restart
            .as_ref()
            .ok_or("missing supervised restart intent")?;
        let mut closed = s.clone();
        closed.mode = Mode::Closed;
        Lifecycle { runtime: r }.authority(&closed)?;
        let finish = finishing(state, &r.config);
        match intent.phase {
            RestartPhase::Draining => crate::restart_stop::RestartStop { runtime: r }.drain(s),
            RestartPhase::Stopping | RestartPhase::Finishing => {
                crate::restart_stop::RestartStop { runtime: r }.observe(state)
            }
            RestartPhase::StartPending => {
                if !r.systemd.gone()? || !(GpuProbe { config: &r.config }).pids()?.is_empty() {
                    return Err("unexpected backend before supervised restart start".into());
                }
                if finish {
                    return crate::shutdown::Shutdown { runtime: r }.stopping(s);
                }
                crate::unit_definition::UnitDefinition { config: &r.config }.verify()?;
                GpuProbe { config: &r.config }.placement()?;
                self.phase(RestartPhase::Starting)?;
                // Persisted exactly once. Startup inspection never resubmits this effect.
                r.systemd.start()
            }
            RestartPhase::Starting => {
                crate::restart_start::RestartStart { runtime: r }.observe(state)
            }
            RestartPhase::Completed => Err("completed restart in restarting state".into()),
        }
    }
    pub fn phase(&self, next: RestartPhase) -> Result<(), String> {
        self.runtime.store.update(|s| {
            let intent = s.service.restart.as_mut().ok_or("missing restart intent")?;
            intent.phase = next;
            intent.since = now();
            Ok(())
        })
    }
}
pub fn finishing(state: &MediaState, config: &crate::config::Config) -> bool {
    !state.sessions.iter().any(|s| {
        Some(&s.id) == state.service.session.as_ref()
            && !s.release_requested
            && !s.stopped
            && now() <= s.created_at + config.session_seconds
    })
}
