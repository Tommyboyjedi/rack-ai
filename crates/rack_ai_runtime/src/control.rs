use crate::{
    service::{Service, owned},
    types::*,
};
use serde::Deserialize;
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub generation: String,
    pub action: Action,
}
#[derive(Clone, Deserialize)]
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
        self.service.authority.update(|s| self.apply(s, c))
    }
    pub(crate) fn apply(&self, s: &mut Document, c: ControlContext<'_>) -> Result<Demand, String> {
        let current = owned(s, c.owner, c.id)?;
        if current.state == DemandState::Unavailable {
            return Ok(current.clone());
        }
        if current.generation != c.request.generation {
            return Err("stale_generation".into());
        }
        if current.recovery_reconciliation.is_some()
            && !matches!(c.request.action, Action::Renew { .. })
        {
            return Ok(current.clone());
        }
        let d = s.data.demands.get_mut(c.id).ok_or("not_found")?;
        match c.request.action {
            Action::Renew { ttl_seconds } => {
                if d.released
                    || d.deadline <= now()
                    || matches!(
                        d.state,
                        DemandState::Unavailable
                            | DemandState::Preempted
                            | DemandState::RecoveryRequired
                    )
                {
                    return Err("reservation_terminal".into());
                }
                if ttl_seconds == 0 || ttl_seconds > self.service.config.max_ttl_seconds {
                    return Err("invalid_ttl".into());
                }
                d.deadline = now() + ttl_seconds;
            }
            Action::Release | Action::Cancel => {
                d.released = true;
                d.reservation_closed = Some(if matches!(c.request.action, Action::Cancel) {
                    DemandState::Cancelled
                } else {
                    DemandState::Released
                });
                if d.state == DemandState::RecoveryRequired {
                    let retrying_cleanup = d.recovery_error.is_some();
                    d.recovery_error = None;
                    d.transition_deadline = now()
                        + if retrying_cleanup {
                            0
                        } else {
                            d.profile.drain_seconds
                        };
                } else {
                    d.transition_deadline = now() + d.profile.drain_seconds;
                }
                if !matches!(
                    d.state,
                    DemandState::Unavailable | DemandState::RecoveryRequired
                ) {
                    d.state = DemandState::Releasing;
                }
                if d.state != DemandState::RecoveryRequired
                    && d.reason.as_deref() != Some(crate::idle::IDLE_TIMEOUT)
                {
                    d.reason = Some(
                        match c.request.action {
                            Action::Cancel => "cancelled",
                            _ => "released",
                        }
                        .into(),
                    );
                }
                for i in s.data.invocations.values_mut().filter(|i| {
                    i.request.reservation_id == c.id
                        && (i.state == InvocationState::Queued
                            || matches!(c.request.action, Action::Cancel))
                }) {
                    i.cancel();
                }
            }
        }
        Ok(d.clone())
    }
    pub fn cancel_invocation(&self, owner: &str, id: &str) -> Result<Invocation, String> {
        self.service.authority.update(|s| {
            let i = s
                .data
                .invocations
                .get_mut(id)
                .filter(|i| i.owner == owner)
                .ok_or("not_found")?;
            i.cancel();
            let result = i.clone();
            crate::workspace_scope::cancel_closed(s);
            Ok(result)
        })
    }
}

pub fn reservation_control(service: &Service, input: (&str, &str, Action)) -> Result<(), String> {
    service.authority.update(|s| {
        let (owner, id, action) = input;
        let d = owned(s, owner, id)?;
        let root_id = crate::reservation::root(s, d)?.id.clone();
        let ids = crate::reservation::members(s, d)?;
        if !matches!(action, Action::Renew { .. }) {
            s.data
                .demands
                .get_mut(&root_id)
                .ok_or("not_found")?
                .reservation_closed = Some(if matches!(action, Action::Cancel) {
                DemandState::Cancelled
            } else {
                DemandState::Released
            });
        }
        for id in ids {
            let generation = owned(s, owner, &id)?.generation.clone();
            (ReservationControl { service }).apply(
                s,
                ControlContext {
                    owner,
                    id: &id,
                    request: Control {
                        generation,
                        action: action.clone(),
                    },
                },
            )?;
        }
        crate::workspace_scope::cancel_closed(s);
        Ok(())
    })
}
