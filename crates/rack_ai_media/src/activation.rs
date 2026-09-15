use crate::{gpu::GpuProbe, lifecycle::Lifecycle, runtime::Runtime, types::*};
use rack_ai_infrastructure::resource_reservations::ReservationRequest;
pub struct Activation<'a> {
    pub runtime: &'a Runtime,
}
impl Activation<'_> {
    pub fn begin(&self, mode: Mode) -> Result<(), String> {
        let r = self.runtime;
        let gpu = GpuProbe { config: &r.config };
        gpu.placement()?;
        gpu.headroom()?;
        if !gpu.pids()?.is_empty() {
            return self.waiting("foreign process on media GPU");
        }
        if !r.systemd.gone()? {
            return Err("unowned candidate service is active".into());
        }
        let activation = identity();
        r.store.update(|s| {
            let session = s
                .sessions
                .iter()
                .find(|x| !x.stopped && !x.release_requested)
                .map(|x| x.id.clone());
            s.service = Service {
                session: if mode == Mode::Interactive {
                    session
                } else {
                    None
                },
                state: ServiceState::Reserving,
                mode,
                activation: activation.clone(),
                since: now(),
                heartbeat: now(),
                last_busy: now(),
                ..Service::default()
            };
            Ok(())
        })?;
        let handle = match r.reservations.acquire(&ReservationRequest {
            owner: format!("media:{activation}"),
            resources: vec!["gpu-4080-super".into()],
            acquired_at: now().to_string(),
            worker_ids: vec![],
            model_ids: if mode == Mode::Managed {
                vec![r.config.profile.checkpoint_sha256.clone()]
            } else {
                vec![]
            },
        }) {
            Ok(h) => h,
            Err(e) if e.starts_with("resource busy:") => {
                return self.waiting("media resource reserved");
            }
            Err(e) => return Err(e),
        };
        // The durable handle precedes startup. Uncertain persistence keeps a blocking remnant.
        r.store.update(|s| {
            s.service.lease = Some(handle);
            s.service.state = ServiceState::Starting;
            Ok(())
        })?;
        let mut service = r.store.read()?.service;
        service.mode = Mode::Closed;
        Lifecycle { runtime: r }.authority(&service)?;
        r.systemd.start()
    }
    fn waiting(&self, reason: &str) -> Result<(), String> {
        self.runtime.store.update(|s| {
            s.service.state = ServiceState::Waiting;
            s.service.error = Some(reason.into());
            Ok(())
        })
    }
}
