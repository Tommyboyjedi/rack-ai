use crate::{
    backend_generation::BackendGeneration,
    config::Principal,
    gpu::GpuProbe,
    lifecycle::Lifecycle,
    process_identity::ProcessIdentity,
    restart_state::{RestartIntent, RestartPhase},
    runtime::Runtime,
    types::*,
};
pub fn is_restart_path(path: &str) -> bool {
    matches!(
        path.strip_prefix("/api").unwrap_or(path),
        "/v2/manager/reboot"
    )
}
pub struct RestartRequest<'a> {
    pub runtime: &'a Runtime,
}
impl RestartRequest<'_> {
    pub fn begin(&self, principal: &Principal) -> Result<String, String> {
        let r = self.runtime;
        let _operation = crate::operation::lock(&r.store.root)?;
        let state = r.store.read()?;
        let service = &state.service;
        if !principal.operator
            || service.mode != Mode::Interactive
            || !state.sessions.iter().any(|s| {
                Some(&s.id) == service.session.as_ref()
                    && s.owner == principal.id
                    && !s.stopped
                    && !s.release_requested
            })
        {
            return Err("restart requires the owning interactive session".into());
        }
        if service.state == ServiceState::Restarting {
            return service
                .restart
                .as_ref()
                .map(|i| i.id.clone())
                .ok_or("missing restart intent".into());
        }
        if service.state != ServiceState::Ready {
            return Err("interactive session is not ready".into());
        }
        crate::unit_definition::UnitDefinition { config: &r.config }.verify()?;
        Lifecycle { runtime: r }.verify(service)?;
        let observed = r.systemd.observe()?;
        ProcessIdentity { config: &r.config }.verify(&observed)?;
        GpuProbe { config: &r.config }.placement()?;
        let previous = BackendGeneration::capture(
            service.generation.as_ref().map_or(1, |g| g.number),
            &observed,
        )?;
        let id = identity();
        r.store.update(|s| {
            s.service.generation = Some(previous.clone());
            s.service.restart = Some(RestartIntent {
                id: id.clone(),
                phase: RestartPhase::Draining,
                since: now(),
                previous,
                target: None,
            });
            s.service.state = ServiceState::Restarting;
            s.service.since = now();
            s.service.error = None;
            Ok(id)
        })
    }
}
