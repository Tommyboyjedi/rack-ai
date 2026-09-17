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
    d.state == DemandState::Ready && active(d) && owns(s, d)
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

/// Mark a reservation usable. Multi-service reservations publish readiness only
/// when every member has safely transferred ownership, started, and independently
/// passed its backend readiness check. Until then every member remains preparing
/// and no scoped gateway can submit work.
pub fn commit_ready(s: &mut Document, d: &Demand) -> Result<(), String> {
    let root = root(s, d)?.clone();
    let ids = members(s, &root)?;
    let atomic = root
        .reserve_request
        .as_ref()
        .is_some_and(|request| request.services.len() > 1);
    if atomic
        && !ids.iter().all(|id| {
            s.data.demands.get(id).is_some_and(|member| {
                member.state == DemandState::Preparing
                    && member.ready_checked
                    && member.process.is_some()
                    && active(member)
                    && owns(s, member)
            })
        })
    {
        return Ok(());
    }
    for id in ids {
        let member = s
            .data
            .demands
            .get_mut(&id)
            .ok_or("missing_reservation_member")?;
        if member.state == DemandState::Preparing && member.ready_checked {
            member.state = DemandState::Ready;
        }
    }
    Ok(())
}
