use crate::{config::Config, types::*};
use rack_ai_infrastructure::managed_authority::ManagedAuthority;
pub struct Service {
    pub config: Config,
    pub gateway_waiters: std::sync::Arc<tokio::sync::Semaphore>,
    pub admission_slots: std::sync::Arc<tokio::sync::Semaphore>,
    pub control_slots: std::sync::Arc<tokio::sync::Semaphore>,
    history_maintenance_after: std::sync::atomic::AtomicU64,
    history_maintenance_running: std::sync::atomic::AtomicBool,
    pub authority: ManagedAuthority<State>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRetirementReport {
    pub archived_invocations: usize,
    pub archived_reservations: usize,
    pub expired_archives: usize,
}
impl Service {
    pub fn new(config: Config) -> Self {
        Self {
            gateway_waiters: std::sync::Arc::new(tokio::sync::Semaphore::new(
                config.limits.max_gateway_waiters,
            )),
            admission_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(16)),
            control_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(16)),
            history_maintenance_after: std::sync::atomic::AtomicU64::new(0),
            history_maintenance_running: std::sync::atomic::AtomicBool::new(false),
            authority: ManagedAuthority::new(config.authority_root.clone()),
            config,
        }
    }
    pub fn inspect(&self, owner: &str, id: &str) -> Result<Demand, String> {
        match self.authority.read(|s| owned(s, owner, id).cloned()) {
            Ok(demand) => Ok(demand),
            Err(error) if error == "not_found" => {
                crate::history_archive::lookup_demand(&self.config.authority_root, owner, id)?
                    .ok_or(error)
            }
            Err(error) => Err(error),
        }
    }
    pub fn result(&self, owner: &str, id: &str) -> Result<Invocation, String> {
        match self.authority.read(|s| {
            s.data
                .invocations
                .get(id)
                .filter(|i| i.owner == owner)
                .cloned()
                .ok_or("not_found".into())
        }) {
            Ok(invocation) => {
                crate::payload_store::hydrate_invocation(&self.config.authority_root, invocation)
            }
            Err(error) if error == "not_found" => {
                crate::history_archive::lookup_invocation(&self.config.authority_root, owner, id)?
                    .ok_or(error)
            }
            Err(error) => Err(error),
        }
    }
    pub fn retire_history_once(&self) -> Result<HistoryRetirementReport, String> {
        let root = self.config.authority_root.clone();
        let mut total = crate::history_archive::MaintenanceReport::default();
        for _ in 0..8 {
            let at = now();
            let Some(plan) = self
                .authority
                .read(|s| crate::history_archive::plan_maintenance(&root, s, at))?
            else {
                break;
            };
            let prepared = crate::history_archive::prepare_maintenance(&root, plan)?;
            let report = self.authority.update(|s| {
                crate::history_archive::commit_prepared_maintenance(&root, s, prepared)
            })?;
            let changed = report.archived_invocations > 0
                || report.archived_reservations > 0
                || report.expired_files > 0;
            total.archived_invocations += report.archived_invocations;
            total.archived_reservations += report.archived_reservations;
            total.expired_files += report.expired_files;
            if !changed {
                break;
            }
        }
        Ok(HistoryRetirementReport {
            archived_invocations: total.archived_invocations,
            archived_reservations: total.archived_reservations,
            expired_archives: total.expired_files,
        })
    }

    pub fn request_history_maintenance(&self) {
        self.history_maintenance_after
            .store(0, std::sync::atomic::Ordering::Release);
    }

    pub fn retire_history_best_effort(&self) {
        if self
            .history_maintenance_running
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let result = self.retire_history_once();
        self.history_maintenance_running
            .store(false, std::sync::atomic::Ordering::Release);
        if let Err(error) = result {
            eprintln!("history archive maintenance failed: {error}");
        }
    }

    pub fn history_maintenance_due(&self, at: u64) -> bool {
        if self
            .history_maintenance_running
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return false;
        }
        let next = self
            .history_maintenance_after
            .load(std::sync::atomic::Ordering::Acquire);
        if at < next {
            return false;
        }
        self.history_maintenance_after
            .compare_exchange(
                next,
                at.saturating_add(60),
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
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
        })?;
        self.retire_history_best_effort();
        crate::residency::reconcile_on_start(self)?;
        Ok(())
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
