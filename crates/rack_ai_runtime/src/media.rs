use crate::{config::Config, types::*};
use rack_ai_application::LeaseHandle;
use rack_ai_media::{
    runtime::Runtime,
    types::{Mode, ServiceState},
};
pub struct MediaAdapter<'a> {
    pub config: &'a Config,
}
impl MediaAdapter<'_> {
    pub(crate) fn runtime(&self, d: &Demand) -> Result<Runtime, String> {
        MediaBinding {
            config: self.config,
        }
        .runtime(d)
    }
    pub fn start(&self, d: &Demand) -> Result<Process, String> {
        let r = self.runtime(d)?;
        let resources = rack_ai_infrastructure::resource_reservations::ResourceReservations::new(
            self.config.authority_root.clone(),
        );
        let mut paths = std::collections::BTreeMap::new();
        for id in &d.profile.resources {
            paths.insert(
                id.clone(),
                resources.path(id)?.to_string_lossy().into_owned(),
            );
        }
        let handle = LeaseHandle {
            owner: d.id.clone(),
            generation: d.generation.clone(),
            paths,
        };
        rack_ai_media::shared_activation::SharedActivation { runtime: &r }.begin(
            rack_ai_media::shared_activation::SharedRequest {
                handle,
                owner: d.owner.clone(),
                mode: d.profile.media_mode.unwrap_or(Mode::Interactive),
            },
        )?;
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(d.profile.startup_seconds);
        loop {
            let observed = r.systemd.observe()?;
            if observed.pid != 0 {
                let mut process = crate::process::capture(observed.pid, &d.generation)?;
                process.unit = Some(r.config.unit.clone());
                process.invocation = Some(observed.invocation);
                return Ok(process);
            }
            if std::time::Instant::now() >= deadline {
                return Err("media_start_process_unknown".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    pub fn observe(&self, d: &Demand) -> Result<(ServiceState, Process), String> {
        MediaObservation {
            config: self.config,
        }
        .observe(d)
    }
    pub fn restarting(&self, d: &Demand) -> Result<bool, String> {
        MediaObservation {
            config: self.config,
        }
        .restarting(d)
    }
    pub fn awaiting_identity(&self, d: &Demand) -> Result<bool, String> {
        let service = self.runtime(d)?.store.read()?.service;
        Ok(service.activation == d.generation
            && service.state == ServiceState::Starting
            && service.generation.is_none()
            && service.invocation.is_none())
    }
    pub fn ready(&self, d: &Demand) -> Result<(), String> {
        MediaObservation {
            config: self.config,
        }
        .ready(d)
    }
    pub fn stop(&self, d: &Demand) -> Result<(), String> {
        let r = self.runtime(d)?;
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(d.profile.drain_seconds + d.profile.stop_seconds);
        loop {
            let service = r.store.read()?.service;
            if service.state == ServiceState::Stopped {
                return Ok(());
            }
            if service.activation != d.generation {
                return Err("media_activation_mismatch".into());
            }
            let shutdown = rack_ai_media::shutdown::Shutdown { runtime: &r };
            if service.state == ServiceState::Stopping {
                shutdown.stopping(&service)?;
            } else {
                shutdown.drain(&service)?;
            }
            if std::time::Instant::now() >= deadline {
                return Err("media_drain_or_stop_deadline".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

struct MediaBinding<'a> {
    config: &'a Config,
}
impl MediaBinding<'_> {
    fn runtime(&self, d: &Demand) -> Result<Runtime, String> {
        let config = crate::media_limits::configuration(d)?;
        if !d.profile.native_media() && config.profile.checkpoint_sha256 != d.profile.model {
            return Err("media_model_binding_mismatch".into());
        }
        if config.resource_root != self.config.authority_root
            || d.profile.resources != ["gpu-4080-super"]
            || config.media_uuid != self.config.devices["gpu-4080-super"].uuid
            || config.backend.trim_end_matches('/') != d.profile.endpoint.trim_end_matches('/')
        {
            return Err("media_shared_authority_binding_mismatch".into());
        }
        Runtime::new(config)
    }
}

struct MediaObservation<'a> {
    config: &'a Config,
}
impl MediaObservation<'_> {
    fn runtime(&self, d: &Demand) -> Result<Runtime, String> {
        MediaBinding {
            config: self.config,
        }
        .runtime(d)
    }
    pub fn observe(&self, d: &Demand) -> Result<(ServiceState, Process), String> {
        let r = self.runtime(d)?;
        let service = r.store.read()?.service;
        if service.activation != d.generation {
            return Err("media_activation_mismatch".into());
        }
        let observed = r.systemd.observe()?;
        let mut p = crate::process::capture(observed.pid, &d.generation)?;
        p.unit = Some(r.config.unit.clone());
        p.invocation = Some(observed.invocation);
        Ok((service.state, p))
    }
    pub fn restarting(&self, d: &Demand) -> Result<bool, String> {
        let state = self.runtime(d)?.store.read()?;
        Ok(state.service.activation == d.generation
            && state
                .service
                .restart
                .as_ref()
                .is_some_and(|r| r.phase != rack_ai_media::restart_state::RestartPhase::Completed))
    }
    pub fn ready(&self, d: &Demand) -> Result<(), String> {
        let r = self.runtime(d)?;
        let state = r.store.read()?;
        if state.service.activation != d.generation {
            return Err("media_activation_mismatch".into());
        }
        if state.service.state == ServiceState::Starting {
            rack_ai_media::startup::StartupObservation { runtime: &r }.observe()?;
        }
        let service = r.store.read()?.service;
        if service.state != ServiceState::Ready {
            return Err("media_not_ready".into());
        }
        rack_ai_media::lifecycle::Lifecycle { runtime: &r }.verify(&service)?;
        rack_ai_media::lifecycle::Lifecycle { runtime: &r }.authority(&service)
    }
}
