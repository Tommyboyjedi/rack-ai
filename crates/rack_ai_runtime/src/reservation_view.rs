use crate::{
    service::{Service, owned},
    types::*,
};
use serde_json::{Value, json};
pub fn inspect(service: &Service, input: (&str, &str)) -> Result<Value, String> {
    service.authority.read(|s| {
        let d = owned(s, input.0, input.1)?;
        let root = crate::reservation::root(s, d)?;
        let ids = crate::reservation::members(s, root)?;
        let mut services = serde_json::Map::new();
        let mut states = Vec::new();
        let ready = crate::reservation::ready(s, root);
        for id in ids {
            let member = owned(s, input.0, &id)?;
            states.push(member.state);
            let mut view = crate::api::public(member.clone())?;
            if !ready && let Some(access) = view.get_mut("access") { access["url"] = Value::Null; }
            services.insert(member.profile.tag.clone(), view);
        }
        let state = aggregate(&states);
        Ok(json!({"id":root.id,"priority":root.priority,"state":state,"reason":root.reason,
            "acquisition_id":root.reserve_request.as_ref().map(|r| &r.acquisition_id),"services":services}))
    })
}
fn aggregate(states: &[DemandState]) -> DemandState {
    use DemandState::*;
    for state in [
        Denied,
        RecoveryRequired,
        Releasing,
        Draining,
        Held,
        Preparing,
        Cancelled,
        Expired,
        Released,
    ] {
        if states.contains(&state) {
            return state;
        }
    }
    Ready
}
