//! One public reservation associates existing service activations in the same authority.
use crate::{
    service::{active, owned, owns},
    types::*,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reserve {
    pub acquisition_id: String,
    pub work_id: String,
    pub services: Vec<String>,
    pub priority: Priority,
    pub ttl_seconds: u64,
}
pub fn root<'a>(s: &'a Document, d: &'a Demand) -> Result<&'a Demand, String> {
    match &d.reservation_id {
        Some(id) => owned(s, &d.owner, id),
        None => Ok(d),
    }
}
pub fn members(s: &Document, d: &Demand) -> Result<Vec<String>, String> {
    let parent = root(s, d)?;
    Ok(if parent.services.is_empty() {
        vec![parent.id.clone()]
    } else {
        parent.services.values().cloned().collect()
    })
}
pub fn ready(s: &Document, d: &Demand) -> bool {
    members(s, d).is_ok_and(|ids| {
        ids.iter().all(|id| {
            s.data
                .demands
                .get(id)
                .is_some_and(|d| d.state == DemandState::Ready && active(d) && owns(s, d))
        })
    })
}
pub fn select<'a>(s: &'a Document, input: (&str, &str, &str)) -> Result<&'a Demand, String> {
    let (owner, id, tag) = input;
    let parent = owned(s, owner, id)?;
    if parent
        .reservation_id
        .as_deref()
        .is_some_and(|root| root != id)
    {
        return Err("reservation_id_required".into());
    }
    if parent.services.is_empty() {
        return if parent.profile.tag == tag {
            Ok(parent)
        } else {
            Err("service_not_reserved".into())
        };
    }
    owned(
        s,
        owner,
        parent.services.get(tag).ok_or("service_not_reserved")?,
    )
}

// A single ordinary arbitration decision covers the union before any activation starts.
pub fn admit(
    s: &mut Document,
    input: (&crate::service::Service, &mut [Demand]),
) -> Result<(), String> {
    let (service, demands) = input;
    let mut combined = demands.first().ok_or("invalid_request")?.clone();
    combined.profile.resources = demands
        .iter()
        .flat_map(|d| d.profile.resources.clone())
        .collect();
    combined.profile.host_mib = demands
        .iter()
        .fold(0_u64, |total, d| total.saturating_add(d.profile.host_mib));
    let refusal = demands
        .iter()
        .find_map(crate::admission::eligibility)
        .or_else(|| (crate::planner::Planner { service }).refusal(s, &combined));
    if let Some(reason) = refusal {
        for d in demands {
            d.state = DemandState::Denied;
            d.reason = Some(reason.clone());
        }
        return Ok(());
    }
    crate::planner::fence(s, &mut combined)?;
    for d in demands {
        d.victims = combined.victims.clone();
        d.transition_deadline = combined.transition_deadline.max(
            now()
                + d.profile.startup_seconds
                + d.victims
                    .iter()
                    .filter_map(|id| s.data.demands.get(id))
                    .map(|v| v.profile.drain_seconds + v.profile.stop_seconds)
                    .sum::<u64>(),
        );
        if d.id == combined.id {
            d.generation = combined.generation.clone();
            d.access_key = combined.access_key.clone();
        }
        for resource in &d.profile.resources {
            s.claims.insert(resource.clone(), d.id.clone());
        }
    }
    Ok(())
}
pub fn startable(s: &Document, d: &Demand) -> bool {
    d.reservation_id
        .as_ref()
        .and_then(|id| s.data.demands.get(id))
        .is_none_or(|root| {
            root.id == d.id
                || root.state != DemandState::Preparing
                || root.victims != d.victims
                || d.victims.iter().all(|id| {
                    s.data
                        .demands
                        .get(id)
                        .is_some_and(|v| v.state == DemandState::Held)
                })
        })
}
pub fn close_members(s: &mut Document) {
    let roots: Vec<_> = s
        .data
        .demands
        .values()
        .filter(|d| !d.services.is_empty())
        .map(|d| d.id.clone())
        .collect();
    for root in roots {
        let Some(parent) = s.data.demands.get(&root) else {
            continue;
        };
        let ids: Vec<_> = parent.services.values().cloned().collect();
        let closed = ids
            .iter()
            .any(|id| s.data.demands.get(id).is_none_or(|d| !active(d)));
        if closed {
            for id in ids {
                if let Some(d) = s.data.demands.get_mut(&id)
                    && active(d)
                    && d.state != DemandState::Denied
                {
                    d.released = true;
                    d.state = DemandState::Releasing;
                    d.reason = Some("reservation_service_closed".into());
                    d.transition_deadline = now() + d.profile.drain_seconds;
                }
            }
        }
    }
}
