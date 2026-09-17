use crate::{config::Config, types::*};
use rack_ai_infrastructure::managed_authority::ManagedAuthority;
pub struct Service {
    pub config: Config,
    pub gateway_waiters: std::sync::Arc<tokio::sync::Semaphore>,
    pub admission_slots: std::sync::Arc<tokio::sync::Semaphore>,
    pub control_slots: std::sync::Arc<tokio::sync::Semaphore>,
    pub authority: ManagedAuthority<State>,
}
impl Service {
    pub fn new(config: Config) -> Self {
        Self {
            gateway_waiters: std::sync::Arc::new(tokio::sync::Semaphore::new(
                config.limits.max_gateway_waiters,
            )),
            admission_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(16)),
            control_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(16)),
            authority: ManagedAuthority::new(config.authority_root.clone()),
            config,
        }
    }
    pub fn inspect(&self, owner: &str, id: &str) -> Result<Demand, String> {
        self.authority.read(|s| owned(s, owner, id).cloned())
    }
    pub fn result(&self, owner: &str, id: &str) -> Result<Invocation, String> {
        self.authority.read(|s| {
            s.data
                .invocations
                .get(id)
                .filter(|i| i.owner == owner)
                .cloned()
                .ok_or("not_found".into())
        })
    }
    pub fn recover(&self) -> Result<(), String> {
        let placement =
            digest(&serde_json::to_vec(&self.config.devices).map_err(|e| e.to_string())?);
        self.authority.update(|s| {
            if s.data
                .placement_hash
                .as_ref()
                .is_some_and(|hash| hash != &placement)
            {
                return Err("physical_map_requires_migration".into());
            }
            s.data.placement_hash = Some(placement);
            s.data.gateway_port = Some(
                self.config
                    .listen
                    .parse::<std::net::SocketAddr>()
                    .map_err(|e| e.to_string())?
                    .port(),
            );
            for i in s
                .data
                .invocations
                .values_mut()
                .filter(|i| i.state == InvocationState::Running)
            {
                i.state = InvocationState::Uncertain;
                i.error = Some(
                    "receiver_restart_after_dispatch_intent; never automatically replay".into(),
                );
            }
            for d in s.data.demands.values_mut().filter(|d| {
                d.effect_started && d.process.is_none() && d.state != DemandState::RecoveryRequired
            }) {
                d.state = DemandState::RecoveryRequired;
                d.reason = Some("interrupted_start_requires_owned_process_reconciliation".into());
            }
            Ok(())
        })
    }
}
pub fn owned<'a>(s: &'a Document, owner: &str, id: &str) -> Result<&'a Demand, String> {
    s.data
        .demands
        .get(id)
        .filter(|d| d.owner == owner)
        .ok_or_else(|| "not_found".into())
}
pub fn active(d: &Demand) -> bool {
    !d.released && d.deadline > now()
}
pub fn owns(s: &Document, d: &Demand) -> bool {
    d.profile
        .resources
        .iter()
        .all(|r| s.claims.get(r) == Some(&d.id))
}
pub fn inflight(s: &Document, id: &str) -> bool {
    s.data.invocations.values().any(|i| {
        i.request.reservation_id == id
            && !crate::work_payload::is_workspace(i)
            && matches!(
                i.state,
                InvocationState::Running | InvocationState::Uncertain
            )
    })
}
