use crate::{gpu::GpuProbe, runtime::Runtime, types::*};
pub struct Shutdown<'a> {
    pub runtime: &'a Runtime,
}
impl Shutdown<'_> {
    pub fn drain(&self, service: &Service) -> Result<(), String> {
        let r = self.runtime;
        crate::lifecycle::Lifecycle { runtime: r }.verify(service)?;
        let mut closed = service.clone();
        closed.mode = Mode::Closed;
        crate::lifecycle::Lifecycle { runtime: r }.authority(&closed)?;
        let gate = r.backend.barrier()?;
        if gate.activation != service.activation || gate.mode != Mode::Closed {
            return Err("gate barrier identity mismatch".into());
        }
        if r.backend.queue()?.empty() {
            crate::lifecycle::Lifecycle { runtime: r }.transition(ServiceState::Stopping)?;
            r.systemd.stop(service)?;
        } else if now() > service.since + r.config.drain_timeout {
            return Err("drain deadline; accepted work still active".into());
        }
        Ok(())
    }
    pub fn stopping(&self, service: &Service) -> Result<(), String> {
        let r = self.runtime;
        if r.systemd.gone()? && (GpuProbe { config: &r.config }).pids()?.is_empty() {
            crate::media_purge::purge_configured(&r.config)?;
            let handle = service.lease.as_ref().ok_or("missing ownership at stop")?;
            r.reservations.release(handle)?;
            r.store.update(|state| {
                state.service = Service::default();
                for s in &mut state.sessions {
                    if s.release_requested || Some(&s.id) == service.session.as_ref() {
                        s.stopped = true;
                    }
                }
                for j in &mut state.jobs {
                    if j.activation.as_ref() == Some(&service.activation) && j.cleanup_pending {
                        j.cleanup_pending = false;
                        if !j.state.terminal() {
                            j.state = JobState::Interrupted;
                            j.error = Some("backend stopped".into());
                        }
                    }
                }
                Ok(())
            })?;
        } else if now() > service.since + r.config.stop_timeout {
            return Err("owned process/GPU cleanup deadline".into());
        }
        Ok(())
    }
}
