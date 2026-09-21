//! Stop-only recovery: uncertain history never grants perpetual physical ownership.

pub fn pending(s: &Document) -> bool {
    s.data.demands.values().any(|d| uncertain_owner(s, d))
}

pub fn quarantine(s: &mut Document) {
    let ids = s
        .data
        .demands
        .values()
        .filter(|d| uncertain_owner(s, d))
        .map(|d| d.id.clone())
        .collect::<Vec<_>>();
    for id in ids {
        if let Some(d) = s.data.demands.get_mut(&id) {
            d.state = DemandState::RecoveryRequired;
            d.reason = Some("invocation_outcome_unknown".into());
        }
    }
}

fn uncertain_owner(s: &Document, d: &Demand) -> bool {
    matches!(
        d.state,
        DemandState::Ready | DemandState::Preparing | DemandState::Preempting
    ) && owns(s, d)
        && s.data
            .invocations
            .values()
            .any(|i| i.request.reservation_id == d.id && i.state == InvocationState::Uncertain)
}
use crate::{
    service::{Service, owns},
    types::*,
};

pub const RETRY_SECONDS: u64 = 2;
pub const QUEUED_CANCEL_REASON: &str = "reservation_recovery_cleanup";

pub struct Recovery<'a> {
    pub service: &'a Service,
}
impl Recovery<'_> {
    pub fn advance(&self, demand: &Demand) -> Result<(), String> {
        let d = self.prepare(demand)?;
        if crate::media_evidence::supported(&d) {
            let runtime = crate::media_evidence::runtime(self.service, &d)?;
            let _operation = rack_ai_media::operation::lock(&runtime.store.root)?;
            let process = (crate::recovery_media::MediaCleanup { runtime: &runtime }).run(&d)?;
            return (Reconciliation {
                service: self.service,
            })
            .finish((&d, process));
        }
        let mut cleanup = d.clone();
        let resolved = crate::owned_effect::resolve(&d)?;
        cleanup.process = resolved.clone().or_else(|| d.process.clone());
        let recovered_process = cleanup.process.clone();
        // Persist a rediscovered identity before effects, so a crash repeats
        // verification of that identity instead of inventing a new start.
        self.service.authority.update(|s| {
            fence(s, &d)?;
            crate::transition::current(s, &d)?.process = cleanup.process.clone();
            Ok(())
        })?;
        if resolved.is_some() {
            crate::hosting::Hosting {
                config: &self.service.config,
            }
            .stop(&cleanup)?;
        }
        crate::owned_effect::absent(&cleanup)?;
        crate::gpu_cleanup::wait(&self.service.config, &cleanup.profile)?;
        (Reconciliation {
            service: self.service,
        })
        .finish((&cleanup, recovered_process))
    }

    fn prepare(&self, d: &Demand) -> Result<Demand, String> {
        self.service.authority.update(|s| {
            fence(s, d)?;
            let saved = crate::transition::current(s, d)?;
            saved.released = true;
            saved.transition_deadline = now() + RETRY_SECONDS;
            for i in s
                .data
                .invocations
                .values_mut()
                .filter(|i| i.request.reservation_id == d.id)
            {
                if i.state == InvocationState::Queued {
                    i.cancel();
                    i.error = Some(QUEUED_CANCEL_REASON.into());
                }
            }
            Ok(())
        })?;
        self.service.authority.read(|s| {
            quiescent(s, d)?;
            s.data
                .demands
                .get(&d.id)
                .cloned()
                .ok_or("missing_transition".into())
        })
    }
}

struct Reconciliation<'a> {
    service: &'a Service,
}
impl Reconciliation<'_> {
    fn finish(&self, input: (&Demand, Option<Process>)) -> Result<(), String> {
        let (d, process) = input;
        self.service.authority.update(|s| {
            quiescent(s, d)?;
            let saved = crate::transition::current(s, d)?;
            let historical_outcome = if matches!(
                saved.reason.as_deref(),
                Some(
                    "start_outcome_unknown"
                        | "interrupted_start_requires_owned_process_reconciliation"
                )
            ) {
                HistoricalOutcome::StartOutcomeUnknown
            } else {
                HistoricalOutcome::RecoveryOutcomeUnknown
            };
            saved.recovery_reconciliation = Some(RecoveryReconciliation {
                historical_outcome,
                current_effect: CurrentEffect::ProvenAbsent,
                reconciled_at: now(),
                cleanup_process: process,
                checks: RecoveryAbsenceChecks {
                    reservation_inactive: true,
                    active_invocations_absent: true,
                    recorded_process_absent: true,
                    systemd_activation_absent: true,
                    gpu_allocation_absent: true,
                    media_session_absent: true,
                    lifecycle_transition_absent: true,
                    ownership_fence_intact: true,
                },
            });
            saved.process = None;
            saved.effect_started = false;
            saved.recovery_error = None;
            saved.state = if saved.preempted_by.is_some() {
                DemandState::Preempted
            } else if saved.reservation_closed == Some(DemandState::Cancelled)
                || saved.reason.as_deref() == Some("cancelled")
            {
                DemandState::Cancelled
            } else if saved.deadline <= now()
                || saved.reason.as_deref() == Some(crate::idle::IDLE_TIMEOUT)
            {
                DemandState::Expired
            } else {
                DemandState::Released
            };
            s.claims.retain(|_, owner| owner != &d.id);
            Ok(())
        })
    }
}

fn fence(s: &Document, d: &Demand) -> Result<(), String> {
    let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
    if saved.generation != d.generation
        || saved.profile_hash != d.profile_hash
        || saved.process != d.process
        || saved.state != DemandState::RecoveryRequired
        || !owns(s, saved)
    {
        return Err("recovery_ownership_fence_changed".into());
    }
    if s.data.demands.values().any(|other| {
        other.id != d.id
            && other
                .profile
                .resources
                .iter()
                .any(|r| d.profile.resources.contains(r))
            && (other.process.is_some() || other.effect_started)
            && matches!(
                other.state,
                DemandState::Preparing
                    | DemandState::Ready
                    | DemandState::Preempting
                    | DemandState::Releasing
                    | DemandState::RecoveryRequired
            )
    }) {
        return Err("recovery_overlapping_effect".into());
    }
    Ok(())
}

fn quiescent(s: &Document, d: &Demand) -> Result<(), String> {
    fence(s, d)?;
    if s.data.invocations.values().any(|i| {
        i.request.reservation_id == d.id
            && (matches!(i.state, InvocationState::Queued | InvocationState::Running)
                || (i.state == InvocationState::Uncertain
                    && crate::work_payload::is_workspace(i)
                    && !crate::workspace_recovery::allows_recovery(s, d, i)))
    }) {
        return Err("recovery_active_or_unproven_workspace_invocation".into());
    }
    Ok(())
}
