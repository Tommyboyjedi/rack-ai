use crate::{
    service::{Service, active},
    types::*,
};
use std::sync::Arc;
struct Reconsideration<'a> {
    service: &'a Service,
}
impl Reconsideration<'_> {
    pub fn reconsider(&self) -> Result<(), String> {
        if !self
            .service
            .authority
            .read(|s| Ok(pending_changes(self.service, s)))?
        {
            return Ok(());
        }
        self.service.authority.update(|s| {
            crate::workspace_scope::cancel_closed(s);
            crate::recovery::quarantine(s);
            crate::activity_retention::refresh(
                s,
                (self.service.config.idle_timeout_seconds, now()),
            )?;
            crate::idle::reap(s, self.service.config.idle_timeout_seconds, now());
            // If the incoming candidate is cancelled, expires, or fails while
            // its incumbent is draining, there is deliberately no restoration.
            // Complete the incumbent's retirement as a terminal preemption so a
            // stranded Preempting record cannot retain a physical claim forever.
            let abandoned_preemptors = s
                .data
                .demands
                .values()
                .filter(|candidate| {
                    !candidate.victims.is_empty() && candidate.state != DemandState::Preparing
                })
                .map(|candidate| candidate.id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            for demand in s.data.demands.values_mut() {
                if demand.state == DemandState::Preempting
                    && demand
                        .preempted_by
                        .as_ref()
                        .is_some_and(|id| abandoned_preemptors.contains(id))
                {
                    demand.released = true;
                    demand.state = DemandState::Releasing;
                    demand.reason = Some("preempted_by_higher_priority".into());
                    demand.transition_deadline = now() + demand.profile.drain_seconds;
                }
            }
            for d in s.data.demands.values_mut() {
                let receipt = self
                    .service
                    .config
                    .authority_root
                    .join("managed-releases")
                    .join(format!("{}.json", d.generation));
                if receipt.exists() {
                    let handle: rack_ai_application::LeaseHandle = serde_json::from_str(
                        &std::fs::read_to_string(receipt).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    if handle.owner != d.id || handle.generation != d.generation {
                        return Err("media_release_receipt_mismatch".into());
                    }
                    d.released = true;
                }
                if !active(d)
                    && matches!(
                        d.state,
                        DemandState::Ready | DemandState::Preparing | DemandState::Preempting
                    )
                {
                    d.state = DemandState::Releasing;
                    d.transition_deadline = now() + d.profile.drain_seconds;
                }
            }
            for i in s.data.invocations.values_mut() {
                if i.state == InvocationState::Queued
                    && (i.waiting_deadline <= now()
                        || !s
                            .data
                            .demands
                            .get(&i.request.reservation_id)
                            .is_some_and(active))
                {
                    i.state = InvocationState::Expired;
                }
            }
            Ok(())
        })?;
        self.service.retire_history_best_effort();
        Ok(())
    }
}
pub struct Supervisor {
    pub service: Arc<Service>,
}
impl Supervisor {
    pub fn tick(&self) -> Result<(), String> {
        Reconsideration {
            service: &self.service,
        }
        .reconsider()?;
        let (mut demands, invocations) = self.service.authority.read(|s| {
            let mut reservations = std::collections::BTreeSet::new();
            Ok((
                s.data
                    .demands
                    .values()
                    .filter(|d| {
                        matches!(
                            d.state,
                            DemandState::Preparing | DemandState::Ready | DemandState::Releasing
                        ) || recovery_due(s, d)
                    })
                    .filter(|d| {
                        d.state == DemandState::RecoveryRequired
                            || !s.data.demands.values().any(|parent| {
                                parent.state == DemandState::Preparing
                                    && parent.victims.contains(&d.id)
                            })
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
                s.data
                    .invocations
                    .values()
                    .filter(|i| {
                        crate::workers::eligible(s, i)
                            && crate::dispatch::workspace_slot(&self.service, s, i)
                    })
                    .filter(|i| reservations.insert(i.request.reservation_id.clone()))
                    .map(|i| i.id.clone())
                    .collect::<Vec<_>>(),
            ))
        })?;
        demands.sort_by_key(|d| {
            (
                d.state != DemandState::Releasing,
                d.state == DemandState::Ready,
            )
        });
        for d in demands {
            let Some(permit) =
                crate::workers::permit(&self.service, crate::workers::Pool::Transition)?
            else {
                break;
            };
            let Some(lock) = crate::workers::record_lock(&self.service, &d.id)? else {
                continue;
            };
            let r = Arc::clone(&self.service);
            std::thread::spawn(move || {
                let _permit = permit;
                let _lock = lock;
                let fresh = match r.inspect(&d.owner, &d.id) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("transition read failed: {e}");
                        return;
                    }
                };
                if let Err(error) = (crate::transition::Transition { service: &r }).advance(&fresh)
                {
                    let saved = r.authority.update(|s| {
                        let current = crate::transition::current(s, &fresh)?;
                        // A late read-only probe or rejected boundary must not erase a release.
                        if (fresh.state == DemandState::Ready
                            && current.state != DemandState::Ready)
                            // A second group-member callback can race the
                            // atomic Ready publication. Once another callback
                            // has moved this record out of Preparing, its stale
                            // transition is harmless evidence, not recovery.
                            || (error == "transition_fence_changed"
                                && current.state != DemandState::Preparing)
                        {
                            return Ok(());
                        }
                        if current.state != DemandState::RecoveryRequired {
                            current.state = DemandState::RecoveryRequired;
                            current.reason = Some(crate::capacity::diagnostic(error.clone()));
                        }
                        current.recovery_error = Some(crate::capacity::diagnostic(error));
                        current.transition_deadline = now() + crate::recovery::RETRY_SECONDS;
                        Ok(())
                    });
                    if let Err(e) = saved {
                        eprintln!("transition evidence persistence failed: {e}");
                    }
                }
            });
        }
        for id in invocations {
            let Some(permit) =
                crate::workers::permit(&self.service, crate::workers::Pool::Dispatch)?
            else {
                break;
            };
            let Some(lock) = crate::workers::record_lock(&self.service, &id)? else {
                continue;
            };
            let r = Arc::clone(&self.service);
            std::thread::spawn(move || {
                let _permit = permit;
                let _lock = lock;
                if let Err(e) = (crate::dispatch::Dispatch { service: &r }).run(&id) {
                    eprintln!("dispatch blocked: {e}");
                }
            });
        }
        Ok(())
    }
}

// This read-only hint avoids locking/serializing stable retained history every tick.
// Every mutation and priority/ownership decision is rechecked under update's lock.
fn pending_changes(service: &Service, s: &Document) -> bool {
    crate::activity_retention::pending(s, (service.config.idle_timeout_seconds, now()))
        || crate::recovery::pending(s)
        || crate::idle::pending(s, service.config.idle_timeout_seconds, now())
        || crate::history_archive::pending(&service.config.authority_root, s, now())
        || s.data.invocations.values().any(|i| {
            crate::workspace_scope::needs_cancel(s, i)
                || i.state == InvocationState::Queued
                    && (i.waiting_deadline <= now()
                        || !s
                            .data
                            .demands
                            .get(&i.request.reservation_id)
                            .is_some_and(active))
        })
        || s.data.demands.values().any(|d| {
            (!d.released
                && service
                    .config
                    .authority_root
                    .join("managed-releases")
                    .join(format!("{}.json", d.generation))
                    .exists())
                || (!active(d)
                    && matches!(
                        d.state,
                        DemandState::Ready | DemandState::Preparing | DemandState::Preempting
                    ))
                || d.state == DemandState::Preempting
                || recovery_due(s, d)
        })
}

fn recovery_due(s: &Document, d: &Demand) -> bool {
    if d.state != DemandState::RecoveryRequired {
        return false;
    }
    let start_recovery = matches!(
        d.reason.as_deref(),
        Some("start_outcome_unknown" | "interrupted_start_requires_owned_process_reconciliation")
    );
    if !start_recovery && d.transition_deadline > now() {
        return false;
    }
    let restart_uncertain = s.data.invocations.values().any(|i| {
        i.request.reservation_id == d.id
            && i.state == InvocationState::Uncertain
            && i.error
                .as_deref()
                .is_some_and(|e| e.starts_with("receiver_restart_after_dispatch_intent"))
    });
    !restart_uncertain || !active(d) || d.released || d.reservation_closed.is_some()
}
