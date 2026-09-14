//! Called only by RackAI's authenticated runtime authority with a durable grant.
use crate::{runtime::Runtime, types::*};
use rack_ai_application::LeaseHandle;
use rack_ai_infrastructure::managed_lease::ManagedLease;
pub struct SharedActivation<'a> {
    pub runtime: &'a Runtime,
}
pub struct SharedRequest {
    pub handle: LeaseHandle,
    pub owner: String,
    pub mode: Mode,
}
impl SharedActivation<'_> {
    pub fn begin(&self, request: SharedRequest) -> Result<(), String> {
        let r = self.runtime;
        if request
            .handle
            .paths
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != ["gpu-4080-super"]
            || !(ManagedLease {
                resources: &r.reservations,
            })
            .verify(&request.handle, false)?
        {
            return Err("shared media requires the complete managed grant".into());
        }
        // Replace only dedicated placement assumptions; retain actual identity,
        // headroom, GPU cleanliness, native gate, process, artifact and restart logic.
        crate::shared_placement::verify(&r.config)?;
        let gpu = crate::gpu::GpuProbe { config: &r.config };
        gpu.headroom()?;
        if !gpu.pids()?.is_empty() || !r.systemd.gone()? {
            return Err("shared media cleanup unproven".into());
        }
        let activation = request.handle.generation.clone();
        r.store.update(|s| {
            if !matches!(
                s.service.state,
                ServiceState::Stopped | ServiceState::Waiting
            ) || s.jobs.iter().any(|j| !j.state.terminal())
                || s.sessions
                    .iter()
                    .any(|x| !x.stopped && !x.release_requested)
            {
                return Err("existing media demand requires migration".into());
            }
            let session = Session {
                id: identity(),
                owner: request.owner,
                request: SessionRequest {
                    schema: VERSION.into(),
                    idempotency_key: request.handle.owner.clone(),
                },
                created_at: now(),
                release_requested: false,
                stopped: false,
            };
            s.service = Service {
                session: if request.mode == Mode::Interactive {
                    Some(session.id.clone())
                } else {
                    None
                },
                state: ServiceState::Starting,
                mode: request.mode,
                activation,
                lease: Some(request.handle),
                since: now(),
                last_busy: now(),
                ..Service::default()
            };
            if request.mode == Mode::Interactive {
                s.sessions.push(session);
            }
            Ok(())
        })?;
        let mut service = r.store.read()?.service;
        service.mode = Mode::Closed;
        crate::lifecycle::Lifecycle { runtime: r }.authority(&service)?;
        r.systemd.start()
    }
}
