use crate::{
    service::{Service, owned},
    types::*,
};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub generation: String,
    pub action: Action,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Renew { ttl_seconds: u64 },
    Release,
    Cancel,
}
pub struct ControlContext<'a> {
    pub owner: &'a str,
    pub id: &'a str,
    pub request: Control,
}
pub struct ReservationControl<'a> {
    pub service: &'a Service,
}
impl ReservationControl<'_> {
    pub fn control(&self, c: ControlContext<'_>) -> Result<Demand, String> {
        self.service.authority.update(|s| {
            let current = owned(s, c.owner, c.id)?;
            if current.state == DemandState::Denied {
                return Ok(current.clone());
            }
            if current.generation != c.request.generation {
                return Err("stale_generation".into());
            }
            let d = s.data.demands.get_mut(c.id).ok_or("not_found")?;
            match c.request.action {
                Action::Renew { ttl_seconds } => {
                    if d.released || d.deadline <= now() || d.state == DemandState::Denied {
                        return Err("reservation_terminal".into());
                    }
                    if ttl_seconds == 0 || ttl_seconds > self.service.config.max_ttl_seconds {
                        return Err("invalid_ttl".into());
                    }
                    d.deadline = now() + ttl_seconds;
                }
                Action::Release | Action::Cancel => {
                    d.released = true;
                    d.transition_deadline = now() + d.profile.drain_seconds;
                    if d.state != DemandState::Denied {
                        d.state = DemandState::Releasing;
                    }
                    d.reason = Some(
                        match c.request.action {
                            Action::Cancel => "cancelled",
                            _ => "released",
                        }
                        .into(),
                    );
                    for i in s.data.invocations.values_mut().filter(|i| {
                        i.request.reservation_id == c.id && i.state == InvocationState::Accepted
                    }) {
                        i.state = InvocationState::Cancelled;
                    }
                }
            }
            Ok(d.clone())
        })
    }
    pub fn cancel_invocation(&self, owner: &str, id: &str) -> Result<Invocation, String> {
        self.service.authority.update(|s| {
            let i = s
                .data
                .invocations
                .get_mut(id)
                .filter(|i| i.owner == owner)
                .ok_or("not_found")?;
            if i.state == InvocationState::Accepted {
                i.state = InvocationState::Cancelled;
            } else if i.state == InvocationState::Started {
                i.error =
                    Some("cancellation_requested_after_start; bounded drain continues".into());
            }
            Ok(i.clone())
        })
    }
}
