use crate::{
    service::{Service, active},
    types::*,
};
use std::{fs::OpenOptions, sync::Arc};
struct Reconsideration<'a> {
    service: &'a Service,
}
impl Reconsideration<'_> {
    pub fn reconsider(&self) -> Result<(), String> {
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
                if i.state == InvocationState::Accepted && i.deadline <= now() {
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
        let (demands, invocations) = self.service.authority.update(|s| {
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
                    .filter(|i| i.state == InvocationState::Accepted)
                    .map(|i| i.id.clone())
                    .collect::<Vec<_>>(),
            ))
        })?;
        for d in demands {
            let Some(lock) = self.worker_lock(&d.id)? else {
                continue;
            };
            let r = Arc::clone(&self.service);
            std::thread::spawn(move || {
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
                        current.reason = Some(error);
                        Ok(())
                    });
                    if let Err(e) = saved {
                        eprintln!("transition evidence persistence failed: {e}");
                    }
                }
            });
        }
        for id in invocations {
            let Some(lock) = self.worker_lock(&id)? else {
                continue;
            };
            let r = Arc::clone(&self.service);
            std::thread::spawn(move || {
                let _lock = lock;
                if let Err(e) = (crate::dispatch::Dispatch { service: &r }).run(&id) {
                    eprintln!("dispatch blocked: {e}");
                }
            });
        }
        Ok(())
    }
    fn worker_lock(&self, id: &str) -> Result<Option<std::fs::File>, String> {
        let root = self.service.config.authority_root.join("workers");
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(id))
            .map_err(|e| e.to_string())?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}
