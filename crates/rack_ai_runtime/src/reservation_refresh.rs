//! Explicit re-evaluation of missing services; never part of the supervisor.
use crate::{
    service::{Service, owned},
    types::*,
};
use serde_json::Value;
pub fn refresh(service: &Service, input: (&str, &str)) -> Result<Value, String> {
    service.authority.update(|s| {
        let root = owned(s, input.0, input.1)?.clone();
        if root.reservation_id.as_deref() != Some(input.1) {
            return Err("reservation_id_required".into());
        }
        let request = root
            .reserve_request
            .as_ref()
            .ok_or("reservation_id_required")?;
        if root.reservation_closed.is_some() || root.created + request.ttl_seconds <= now() {
            return Err("reservation_terminal".into());
        }
        for tag in &request.services {
            let id = root.services.get(tag).ok_or("service_not_reserved")?;
            let mut d = owned(s, input.0, id)?.clone();
            if d.state != DemandState::Unavailable || d.released {
                continue;
            }
            if let Some(profile) = service.config.profiles.iter().find(|p| &p.tag == tag) {
                d.profile = profile.clone();
                d.profile_hash = digest(&serde_json::to_vec(profile).map_err(|e| e.to_string())?);
                d.request.capabilities = profile.capabilities.clone();
                d.request.context_tokens = profile.context_tokens;
                crate::reservation_admission::attempt(service, (s, &mut d))?;
            } else {
                d.reason = Some("unknown_tag".into());
            }
            s.data.demands.insert(id.clone(), d);
        }
        crate::capacity::retention(s, &service.config.limits)?;
        crate::reservation_view::view(s, input)
    })
}
