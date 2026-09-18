use crate::{
    service::{Service, inflight},
    types::*,
};
use std::collections::BTreeSet;

pub struct Planner<'a> {
    pub service: &'a Service,
}
impl Planner<'_> {
    /// A refusal is an acquisition decision, never a queueing decision. A
    /// candidate already fencing the same lower-priority victim is allowed to
    /// continue its own durable transition after a restart.
    pub fn refusal(&self, s: &Document, d: &Demand) -> Option<String> {
        match self
            .service
            .authority
            .resources
            .legacy_blocked(&d.profile.resources)
        {
            Ok(ids) if ids.is_empty() => (),
            Ok(ids) => return Some(format!("legacy_ownership_requires_migration:{ids:?}")),
            Err(e) => return Some(format!("ownership_uncertain:{e}")),
        }
        for id in conflicts(s, d) {
            let Some(incumbent) = s.data.demands.get(&id) else {
                return Some("ownership_uncertain".into());
            };
            if d.victims.contains(&id)
                && incumbent.state == DemandState::Preempting
                && incumbent.preempted_by.as_deref() == Some(&d.id)
            {
                continue;
            }
            if incumbent.priority >= d.priority {
                return Some(format!("incumbent_priority:{id}"));
            }
            if incumbent.state != DemandState::Ready {
                return Some(format!("transition_or_recovery_blocked:{id}"));
            }
        }
        let occupied_host: u64 = s
            .data
            .demands
            .values()
            .filter(|x| {
                matches!(
                    x.state,
                    DemandState::Ready
                        | DemandState::Releasing
                        | DemandState::Preparing
                        | DemandState::Preempting
                        | DemandState::RecoveryRequired
                ) && !conflicts(s, d).contains(&x.id)
            })
            .map(|x| x.profile.host_mib)
            .sum();
        if occupied_host.saturating_add(d.profile.host_mib) > self.service.config.host_capacity_mib
        {
            return Some("insufficient_host_memory".into());
        }
        None
    }
}

pub fn conflicts(s: &Document, d: &Demand) -> BTreeSet<String> {
    d.profile
        .resources
        .iter()
        .filter_map(|r| s.claims.get(r))
        .filter(|id| **id != d.id)
        .cloned()
        .collect()
}

/// Begin durable preemption without transferring a physical claim. Existing
/// queued calls are terminally cancelled under the incumbent's ownership;
/// running work continues until its backend callback is known.
pub fn fence(s: &mut Document, d: &mut Demand) -> Result<(), String> {
    let victims = conflicts(s, d);
    for id in &victims {
        let victim = s.data.demands.get_mut(id).ok_or("ownership_uncertain")?;
        if victim.priority >= d.priority || victim.state != DemandState::Ready {
            return Err("transition_or_recovery_blocked".into());
        }
        victim.state = DemandState::Preempting;
        victim.reason = Some("preempted_by_higher_priority".into());
        victim.preempted_by = Some(d.id.clone());
        victim.transition_deadline = now() + victim.profile.drain_seconds;
    }
    for invocation in s
        .data
        .invocations
        .values_mut()
        .filter(|i| victims.contains(&i.request.reservation_id))
    {
        invocation.cancel_queued_as_superseded();
    }
    d.victims = victims.into_iter().collect();
    // An uncontended acquisition must reserve its physical resources under the
    // same durable decision that creates the Preparing record. Otherwise
    // concurrent callers can all observe an empty claim set and create
    // competing preparations. A candidate with victims waits for transfer after
    // the incumbent has drained and its claims have been released.
    if d.victims.is_empty() {
        for resource in &d.profile.resources {
            if let Some(owner) = s.claims.get(resource)
                && owner != &d.id
            {
                return Err("transition_fence_changed".into());
            }
            s.claims.insert(resource.clone(), d.id.clone());
        }
    }
    d.generation = identity()?;
    d.access_key = identity()?;
    d.transition_deadline = now()
        + d.profile.startup_seconds
        + d.victims
            .iter()
            .filter_map(|v| s.data.demands.get(v))
            .map(|v| v.profile.drain_seconds + v.profile.stop_seconds)
            .sum::<u64>();
    d.state = DemandState::Preparing;
    d.reason = None;
    d.retry_after = None;
    d.effect_started = false;
    d.preflight_done = false;
    Ok(())
}

/// A claim transfers only after every affected invocation has drained safely and
/// its previous backend has stopped. This is called under the authority lock.
pub fn transfer(s: &mut Document, d: &Demand) -> Result<(), String> {
    for victim_id in &d.victims {
        let victim = s.data.demands.get(victim_id).ok_or("missing_victim")?;
        if victim.state != DemandState::Preempted || inflight(s, victim_id) {
            return Err("victim_not_safely_preempted".into());
        }
    }
    for resource in &d.profile.resources {
        if let Some(owner) = s.claims.get(resource)
            && owner != &d.id
        {
            return Err("transition_fence_changed".into());
        }
    }
    for resource in &d.profile.resources {
        s.claims.insert(resource.clone(), d.id.clone());
    }
    Ok(())
}

/// Atomically begin a multi-service reservation. All conflicts are evaluated
/// before any incumbent changes state; an unavailable member therefore cannot
/// leave a partial ownership transfer behind.
pub fn fence_group(s: &mut Document, root_id: &str, demands: &mut [Demand]) -> Result<(), String> {
    let mut victims = BTreeSet::new();
    for demand in demands.iter() {
        victims.extend(conflicts(s, demand));
    }
    for victim_id in &victims {
        let victim = s
            .data
            .demands
            .get_mut(victim_id)
            .ok_or("ownership_uncertain")?;
        if victim.state != DemandState::Ready {
            return Err("transition_or_recovery_blocked".into());
        }
        let priority = demands
            .iter()
            .map(|d| d.priority)
            .max()
            .ok_or("invalid_request")?;
        if victim.priority >= priority {
            return Err(format!("incumbent_priority:{victim_id}"));
        }
        victim.state = DemandState::Preempting;
        victim.reason = Some("preempted_by_higher_priority".into());
        victim.preempted_by = Some(root_id.into());
        victim.transition_deadline = now() + victim.profile.drain_seconds;
    }
    for invocation in s
        .data
        .invocations
        .values_mut()
        .filter(|i| victims.contains(&i.request.reservation_id))
    {
        invocation.cancel_queued_as_superseded();
    }
    // A group shares one drain fence. Members without a direct conflicting
    // resource must not begin or claim early while another member is draining:
    // that would turn an atomic reservation into a partial acquisition.
    if victims.is_empty() {
        for demand in demands.iter() {
            for resource in &demand.profile.resources {
                if let Some(owner) = s.claims.get(resource)
                    && owner != &demand.id
                {
                    return Err("transition_fence_changed".into());
                }
                s.claims.insert(resource.clone(), demand.id.clone());
            }
        }
    }
    let shared_victims = victims.into_iter().collect::<Vec<_>>();
    for demand in demands {
        demand.victims = shared_victims.clone();
        demand.generation = identity()?;
        demand.access_key = identity()?;
        demand.transition_deadline = now()
            + demand.profile.startup_seconds
            + demand
                .victims
                .iter()
                .filter_map(|id| s.data.demands.get(id))
                .map(|victim| victim.profile.drain_seconds + victim.profile.stop_seconds)
                .sum::<u64>();
        demand.state = DemandState::Preparing;
        demand.reason = None;
        demand.retry_after = None;
        demand.preempted_by = None;
        demand.ready_checked = false;
        demand.effect_started = false;
        demand.preflight_done = false;
    }
    Ok(())
}
