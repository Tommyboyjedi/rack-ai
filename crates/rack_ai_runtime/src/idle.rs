//! GPU inactivity is independent of lease renewal and control-plane liveness.
use crate::{
    config::{Backend, Driver},
    service::inflight,
    types::*,
};
pub const IDLE_TIMEOUT: &str = "idle_timeout";
pub const fn default_seconds() -> u64 {
    30 * 60
}
pub fn touch(s: &mut Document, id: &str, at: u64) -> Result<(), String> {
    let d = s.data.demands.get_mut(id).ok_or("missing_reservation")?;
    d.last_activity_at = Some(d.last_activity_at.unwrap_or(d.created).max(at));
    Ok(())
}
pub fn due(d: &Demand, seconds: u64, at: u64) -> bool {
    !d.released
        && matches!(d.state, DemandState::Ready | DemandState::Preparing)
        && at.saturating_sub(d.last_activity_at.unwrap_or(d.created)) >= seconds
}
fn eligible(s: &Document, d: &Demand, seconds: u64, at: u64) -> bool {
    due(d, seconds, at) && !inflight(s, &d.id)
        && !s.data.invocations.values().any(|i| i.request.reservation_id == d.id && crate::work_payload::is_workspace(i) && i.state == InvocationState::Running)
        // Native admission must close atomically with the ComfyUI queue barrier.
        && (d.profile.backend != Backend::Comfyui || d.profile.driver == Driver::Fixture)
}
pub fn pending(s: &Document, seconds: u64, at: u64) -> bool {
    s.data.demands.values().any(|d| eligible(s, d, seconds, at))
}
pub fn reap(s: &mut Document, seconds: u64, at: u64) {
    let ids: Vec<_> = s
        .data
        .demands
        .values()
        .filter(|d| eligible(s, d, seconds, at))
        .map(|d| d.id.clone())
        .collect();
    for id in ids {
        if let Some(d) = s.data.demands.get_mut(&id) {
            expire(d, at);
        }
    }
}
pub fn expire(d: &mut Demand, at: u64) {
    d.released = true;
    d.state = DemandState::Releasing;
    d.reason = Some(IDLE_TIMEOUT.into());
    d.transition_deadline = at + d.profile.drain_seconds;
}
