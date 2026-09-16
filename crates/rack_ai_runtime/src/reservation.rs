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
