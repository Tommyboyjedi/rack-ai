use crate::{
    hosting::Hosting,
    service::{Service, inflight},
    transition::current,
    types::*,
};
pub struct Retirement<'a> {
    pub service: &'a Service,
}
impl Retirement<'_> {
    pub fn run(&self, d: &Demand) -> Result<(), String> {
        let r = self.service;
        let (pending, started) = r.authority.read(|s| {
            Ok((
                inflight(s, &d.id),
                s.data.invocations.values().any(|i| {
                    i.request.reservation_id == d.id && i.state == InvocationState::Running
                }),
            ))
        })?;
        // An explicit retirement bounds draining, not the truth of the result.
        // Stop only the verified owned backend; keep claims until the live
        // dispatcher has recorded its late response or uncertain transport result.
        if pending && now() < d.transition_deadline {
            return Ok(());
        }
        if started {
            Hosting { config: &r.config }.stop(d)?;
            return Ok(());
        }
        if d.effect_started && d.process.is_none() {
            return Err("start_outcome_unknown".into());
        }
        Hosting { config: &r.config }.stop(d)?;
        r.authority.update(|s| {
            let saved = current(s, d)?;
            saved.process = None;
            saved.effect_started = false;
            saved.released = true;
            saved.state = match saved.reason.as_deref() {
                Some("cancelled") => DemandState::Cancelled,
                Some("preempted_by_higher_priority") => DemandState::Preempted,
                Some(crate::idle::IDLE_TIMEOUT) => DemandState::Expired,
                _ if saved.deadline <= now() => DemandState::Expired,
                _ => DemandState::Released,
            };
            s.claims.retain(|_, owner| owner != &d.id);
            Ok(())
        })
    }
}
