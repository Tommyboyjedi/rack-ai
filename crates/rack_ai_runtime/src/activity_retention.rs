//! Persist shared activity retention before any deadline-based admission or cleanup.
use crate::{
    service::{inflight, owns},
    types::*,
};

pub fn pending(s: &Document, policy: (u64, u64)) -> bool {
    extensions(s, policy).map_or(true, |updates| !updates.is_empty())
}
pub fn refresh(s: &mut Document, policy: (u64, u64)) -> Result<(), String> {
    for (id, deadline) in extensions(s, policy)? {
        s.data
            .demands
            .get_mut(&id)
            .ok_or("missing_reservation_member")?
            .deadline = deadline;
    }
    Ok(())
}
fn extensions(s: &Document, policy: (u64, u64)) -> Result<Vec<(String, u64)>, String> {
    let (seconds, at) = policy;
    let mut updates = Vec::new();
    for root in s
        .data
        .demands
        .values()
        .filter(|d| d.reserve_request.is_some())
    {
        if root.reservation_closed.is_some() {
            continue;
        }
        let members = crate::reservation::members(s, root)?;
        let peers = members
            .iter()
            .map(|id| crate::service::owned(s, &root.owner, id))
            .collect::<Result<Vec<_>, _>>()?;
        // Retain only currently owned Ready members. Never resurrect an elapsed,
        // released, preempted, failed or partially prepared activation.
        let eligible = |d: &&Demand| {
            !d.released && d.state == DemandState::Ready && d.deadline > at && owns(s, d)
        };
        let activity = peers
            .iter()
            .copied()
            .filter(eligible)
            .filter_map(|d| {
                if inflight(s, &d.id)
                    || s.data.invocations.values().any(|i| {
                        i.request.reservation_id == d.id
                            && crate::work_payload::is_workspace(i)
                            && i.state == InvocationState::Running
                    })
                {
                    Some(at)
                } else {
                    d.last_activity_at
                        .filter(|value| *value >= d.created && *value <= at)
                }
            })
            .max();
        if activity.is_some_and(|activity| at.saturating_sub(activity) < seconds) {
            // Keep the ownership lease ahead of the idle decision, so native
            // admission closes through its queue barrier, not TTL retirement.
            let deadline = at.saturating_add(seconds);
            for d in peers.iter().copied().filter(eligible) {
                if deadline > d.deadline {
                    updates.push((d.id.clone(), deadline));
                }
            }
        }
    }
    Ok(updates)
}
