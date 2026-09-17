use crate::{
    service::{Service, owned},
    types::*,
};
use serde_json::{Value, json};
pub fn inspect(service: &Service, input: (&str, &str)) -> Result<Value, String> {
    service.authority.read(|s| view(s, input))
}
pub(crate) fn view(s: &Document, input: (&str, &str)) -> Result<Value, String> {
    let d = owned(s, input.0, input.1)?;
    let root = crate::reservation::root(s, d)?;
    let ids = crate::reservation::members(s, root)?;
    let mut services = serde_json::Map::new();
    let mut states = Vec::new();
    for id in ids {
        let member = owned(s, input.0, &id)?;
        states.push(member.state);
        let mut view = crate::api::public(member.clone())?;
        if member.state == DemandState::Unavailable {
            view["state"] = json!("unavailable");
            view.as_object_mut()
                .ok_or("invalid_public_record")?
                .remove("gateway_path");
        }
        services.insert(member.profile.tag.clone(), view);
    }
    let state = if let Some(closed) = root.reservation_closed {
        if states.iter().all(|s| {
            matches!(
                s,
                DemandState::Released
                    | DemandState::Cancelled
                    | DemandState::Expired
                    | DemandState::Unavailable
            )
        }) {
            json!(closed)
        } else {
            json!(DemandState::Releasing)
        }
    } else if states.iter().all(|s| *s == DemandState::Unavailable) {
        json!("unavailable")
    } else if states.iter().all(|s| Some(s) == states.first()) {
        json!(states[0])
    } else {
        json!("partial")
    };
    Ok(json!({"id":root.id,"priority":root.priority,"state":state,
        "acquisition_id":root.reserve_request.as_ref().map(|r| &r.acquisition_id),
        "requested_services":root.reserve_request.as_ref().map(|r| &r.services),"services":services}))
}
