use crate::{
    backend_generation::BackendGeneration,
    gate_probe::GateObservation,
    gpu::GpuProbe,
    lifecycle::Lifecycle,
    process_identity::ProcessIdentity,
    restart_lifecycle::{InteractiveRestart, finishing},
    restart_state::*,
    runtime::Runtime,
    types::*,
};
pub struct RestartStart<'a> {
    pub runtime: &'a Runtime,
}
impl RestartStart<'_> {
    pub fn observe(&self, state: &MediaState) -> Result<(), String> {
        let r = self.runtime;
        let s = &state.service;
        let intent = s.restart.as_ref().ok_or("missing restart")?;
        let finish = finishing(state, &r.config);
        let observed = r.systemd.observe()?;
        if observed.pid == 0 {
            if !(GpuProbe { config: &r.config }).pids()?.is_empty() {
                return Err("media GPU process without restart service identity".into());
            }
            if finish {
                if r.systemd.gone()? {
                    return crate::shutdown::Shutdown { runtime: r }.stopping(s);
                }
                r.systemd.cancel_pending_start()?;
                return InteractiveRestart { runtime: r }.phase(RestartPhase::Finishing);
            }
            return self.deadline(intent);
        }
        if observed.invocation == intent.previous.invocation {
            return Err("supervised restart did not create a new unit generation".into());
        }
        ProcessIdentity { config: &r.config }.verify(&observed)?;
        GpuProbe { config: &r.config }.owned(&observed)?;
        GpuProbe { config: &r.config }.placement()?;
        let target = BackendGeneration::capture(
            intent
                .previous
                .number
                .checked_add(1)
                .ok_or("restart generation capacity exceeded")?,
            &observed,
        )?;
        if intent
            .target
            .as_ref()
            .is_some_and(|previous| previous != &target)
        {
            return Err("concurrent replacement during supervised restart startup".into());
        }
        let gate = r.backend.probe_gate()?;
        if let GateObservation::Ready(g) = &gate {
            if g.activation != s.activation
                || g.invocation != target.invocation
                || g.pid != target.pid
                || g.protocol != "rack-gate/v1"
            {
                return Err("restart gate/process identity mismatch".into());
            }
        }
        r.store.update(|state| {
            state
                .service
                .restart
                .as_mut()
                .ok_or("missing restart")?
                .target = Some(target.clone());
            Ok(())
        })?;
        let mut next = s.clone();
        next.invocation = Some(target.invocation.clone());
        next.generation = Some(target.clone());
        if finish {
            InteractiveRestart { runtime: r }.phase(RestartPhase::Finishing)?;
            return r.systemd.stop(&next);
        }
        if matches!(gate, GateObservation::Pending) {
            return self.deadline(intent);
        }
        Lifecycle { runtime: r }.verify(&next)?;
        Lifecycle { runtime: r }.authority(&next)?;
        if r.backend.gate()?.mode != Mode::Interactive {
            return Err("restart gate did not reopen".into());
        }
        r.store.update(|state| {
            state.service.invocation = next.invocation;
            state.service.generation = Some(target);
            state
                .service
                .restart
                .as_mut()
                .ok_or("missing restart")?
                .phase = RestartPhase::Completed;
            state.service.state = ServiceState::Ready;
            state.service.last_busy = now();
            state.service.error = None;
            Ok(())
        })
    }
    fn deadline(&self, intent: &RestartIntent) -> Result<(), String> {
        if now() > intent.since + self.runtime.config.start_timeout {
            return Err("supervised restart startup deadline; backend ownership unconfirmed; no restart retry".into());
        }
        Ok(())
    }
}
