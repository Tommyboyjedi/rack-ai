//! Bounded recovery keeps health failures separate from ambiguous ownership.
use crate::{
    media_evidence::{self, MediaEvidence},
    service::{Service, active},
    types::*,
};
use rack_ai_media::types::{Mode, ServiceState};

pub struct MediaRecovery<'a> {
    pub service: &'a Service,
}
impl MediaRecovery<'_> {
    /// Release only the current claim after every independent absence probe
    /// succeeds. The original start remains historically uncertain in the
    /// durable reconciliation record and is never replayed.
    pub fn reconcile_effect_absent(&self, d: &Demand) -> Result<bool, String> {
        if d.state != DemandState::RecoveryRequired
            || d.reason.as_deref() != Some("start_outcome_unknown")
            || d.process.is_some()
        {
            return Ok(false);
        }
        let runtime = media_evidence::runtime(self.service, d)?;
        let _operation = rack_ai_media::operation::lock(&runtime.store.root)?;
        let checks = MediaEvidence {
            service: self.service,
            runtime: &runtime,
        }
        .effect_absent(d)?;
        self.service.authority.update(|s| {
            media_evidence::absence_authority_document(s, d)?;
            let terminal = {
                let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
                if saved.process.is_some() || !saved.effect_started {
                    return Err("media_recovery_claim_changed".into());
                }
                let root = crate::reservation::root(s, saved)?;
                match root.reservation_closed {
                    Some(DemandState::Cancelled) | None if root.state == DemandState::Cancelled => {
                        DemandState::Cancelled
                    }
                    Some(DemandState::Expired) | None if root.state == DemandState::Expired => {
                        DemandState::Expired
                    }
                    Some(DemandState::Released) | None if root.state == DemandState::Released => {
                        DemandState::Released
                    }
                    _ if saved.deadline <= now() => DemandState::Expired,
                    _ => DemandState::Released,
                }
            };
            let saved = crate::transition::current(s, d)?;
            saved.process = None;
            saved.effect_started = false;
            saved.released = true;
            saved.state = terminal;
            saved.recovery_reconciliation = Some(RecoveryReconciliation {
                cleanup_process: None,
                historical_outcome: HistoricalOutcome::StartOutcomeUnknown,
                current_effect: CurrentEffect::ProvenAbsent,
                reconciled_at: now(),
                checks,
            });
            s.claims.retain(|_, owner| owner != &d.id);
            Ok(())
        })?;
        Ok(true)
    }
    pub fn retire_stopped(&self, d: &Demand) -> Result<bool, String> {
        if self.reconcile_effect_absent(d)? {
            return Ok(true);
        }
        let runtime = media_evidence::runtime(self.service, d)?;
        let operation = rack_ai_media::operation::lock(&runtime.store.root)?;
        let evidence = MediaEvidence {
            service: self.service,
            runtime: &runtime,
        };
        if evidence.stopped(d)? {
            let media = evidence.bound(d)?;
            if media.state != ServiceState::Stopped {
                (rack_ai_media::shutdown::Shutdown { runtime: &runtime }).stopping(&media)?;
            }
            drop(operation);
            (crate::retirement::Retirement {
                service: self.service,
            })
            .run(d)?;
            return Ok(true);
        }
        Ok(false)
    }
    pub fn advance(&self, d: &Demand, failure: Option<&str>) -> Result<(), String> {
        if self.retire_stopped(d)? {
            return Ok(());
        }
        let runtime = media_evidence::runtime(self.service, d)?;
        let _operation = rack_ai_media::operation::lock(&runtime.store.root)?;
        let evidence = MediaEvidence {
            service: self.service,
            runtime: &runtime,
        };
        let media = evidence.owned(d)?;
        if failure.is_some_and(|e| !media_evidence::transport(e) && e != "media_not_ready") {
            return Err(failure.unwrap_or("media_recovery_failure_unknown").into());
        }
        if media.state == ServiceState::RecoveryRequired
            && !media
                .error
                .as_deref()
                .is_some_and(media_evidence::transport)
        {
            return Err("media_recovery_requires_ownership_reconciliation".into());
        }
        if d.state == DemandState::RecoveryRequired
            && !d.reason.as_deref().is_some_and(media_evidence::transport)
        {
            return Err("media_recovery_original_failure_unresolved".into());
        }
        if !active(d) || (d.state == DemandState::Preparing && now() >= d.transition_deadline) {
            let mut closed = media.clone();
            closed.mode = Mode::Closed;
            (rack_ai_media::lifecycle::Lifecycle { runtime: &runtime }).authority(&closed)?;
            runtime.store.update(|s| {
                s.service.state = ServiceState::Stopping;
                s.service.since = now();
                Ok(())
            })?;
            runtime.systemd.stop(&media)?;
            return self.service.authority.update(|s| {
                let saved = crate::transition::current(s, d)?;
                saved.released = true;
                saved.state = DemandState::Releasing;
                Ok(())
            });
        }
        // Reuse startup's bounded health recovery and owned-stop deadline.
        // Keep activation, lease, job history and generation intact; never redispatch.
        if matches!(
            media.state,
            ServiceState::Ready | ServiceState::RecoveryRequired
        ) {
            let mut closed = media.clone();
            closed.mode = Mode::Closed;
            (rack_ai_media::lifecycle::Lifecycle { runtime: &runtime }).authority(&closed)?;
            runtime.store.update(|s| {
                s.service.state = ServiceState::Starting;
                if d.state != DemandState::Preparing {
                    s.service.since = now();
                }
                Ok(())
            })?;
        }
        self.service.authority.update(|s| {
            let saved = crate::transition::current(s, d)?;
            if matches!(
                saved.state,
                DemandState::Ready | DemandState::RecoveryRequired
            ) && active(saved)
            {
                saved.state = DemandState::Preparing;
                saved.transition_deadline = now() + d.profile.startup_seconds;
            } else if !active(saved) {
                saved.state = DemandState::Releasing;
                saved.transition_deadline = now() + d.profile.drain_seconds;
            }
            Ok(())
        })
    }
}
