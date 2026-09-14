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
                        DemandState::Held
                            | DemandState::Ready
                            | DemandState::Preparing
                            | DemandState::Draining
                    )
                {
                    d.state = DemandState::Releasing;
                    d.transition_deadline = now() + d.profile.drain_seconds;
                }
            }
            for i in s.data.invocations.values_mut() {
                if i.state == InvocationState::Accepted
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
            let mut held: Vec<_> = s
                .data
                .demands
                .values()
                .filter(|d| d.state == DemandState::Held && active(d))
                .cloned()
                .collect();
            held.sort_by_key(|d| (std::cmp::Reverse(d.priority), d.order, d.id.clone()));
            for mut d in held {
                if (crate::planner::Planner {
                    service: self.service,
                })
                .refusal(s, &d)
                .is_none()
                {
                    crate::planner::fence(s, &mut d)?;
                    s.data.demands.insert(d.id.clone(), d);
                }
            }
            Ok(())
        })
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
                        ) && !s.data.demands.values().any(|parent| {
                            parent.state == DemandState::Preparing && parent.victims.contains(&d.id)
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
                s.data
                    .invocations
                    .values()
                    .filter(|i| crate::workers::eligible(s, i))
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
                            || (error == "transition_fence_changed"
                                && current.state == DemandState::Releasing)
                        {
                            return Ok(());
                        }
                        current.state = DemandState::RecoveryRequired;
                        current.reason = Some(crate::capacity::diagnostic(error));
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
    s.data.invocations.values().any(|i| {
        i.state == InvocationState::Accepted
            && (i.waiting_deadline <= now()
                || !s
                    .data
                    .demands
                    .get(&i.request.reservation_id)
                    .is_some_and(active))
    }) || s.data.demands.values().any(|d| {
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
                    DemandState::Held
                        | DemandState::Ready
                        | DemandState::Preparing
                        | DemandState::Draining
                ))
            || (d.state == DemandState::Held
                && active(d)
                && (crate::planner::Planner { service })
                    .refusal(s, d)
                    .is_none())
    })
}
