use crate::{gpu::GpuProbe, lifecycle::Lifecycle, runtime::Runtime, types::*};
pub struct StartupObservation<'a> {
    pub runtime: &'a Runtime,
}
impl StartupObservation<'_> {
    pub fn observe(&self) -> Result<(), String> {
        let r = self.runtime;
        let service = r.store.read()?.service;
        let mut closed = service.clone();
        closed.mode = Mode::Closed;
        Lifecycle { runtime: r }.authority(&closed)?;
        let observed = r.systemd.observe()?;
        if let Some(expected) = &service.invocation {
            if expected != &observed.invocation {
                return Err("activation invocation changed".into());
            }
        }
        if !observed.invocation.is_empty() && service.invocation.is_none() {
            // A matching activation in the owned gate is required before adopting an invocation.
            if let Ok(gate) = r.backend.gate() {
                if gate.activation != service.activation
                    || gate.invocation != observed.invocation
                    || gate.pid != observed.pid
                {
                    return Err("unrelated backend on configured port".into());
                }
                GpuProbe { config: &r.config }.owned(&observed)?;
                r.store.update(|s| {
                    s.service.invocation = Some(observed.invocation.clone());
                    s.service.generation = Some(
                        crate::backend_generation::BackendGeneration::capture(1, &observed)?,
                    );
                    Ok(())
                })?;
            }
        }
        let current = r.store.read()?.service;
        if current.invocation.is_some() && (Lifecycle { runtime: r }).verify(&current).is_ok() {
            let released = r
                .store
                .read()?
                .sessions
                .iter()
                .any(|s| Some(&s.id) == current.session.as_ref() && s.release_requested);
            if released {
                return Lifecycle { runtime: r }.transition(ServiceState::Draining);
            }
            Lifecycle { runtime: r }.authority(&current)?;
            let gate = r.backend.gate()?;
            if gate.mode != current.mode {
                return Err("native gate did not open".into());
            }
            return r.store.update(|s| {
                s.service.state = ServiceState::Ready;
                s.service.error = None;
                s.service.last_busy = now();
                Ok(())
            });
        }
        if now() > service.since + r.config.start_timeout {
            if current.invocation.is_some() {
                Lifecycle { runtime: r }.transition(ServiceState::Stopping)?;
                r.systemd.stop(&current)?;
            } else {
                return Err("startup deadline; invocation not verified".into());
            }
        }
        Ok(())
    }
}
