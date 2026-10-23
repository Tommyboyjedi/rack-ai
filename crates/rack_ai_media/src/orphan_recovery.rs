//! Clear only an identity-free, quiescent quarantine after external absence checks.
use crate::{runtime::Runtime, types::*};
pub fn empty(state: &MediaState) -> bool {
    let s = &state.service;
    s.state == ServiceState::RecoveryRequired
        && s.mode == Mode::Closed
        && s.session.is_none()
        && s.activation.is_empty()
        && s.invocation.is_none()
        && s.generation.is_none()
        && s.lease.is_none()
        && s.restart.is_none()
        && state.sessions.iter().all(|s| s.stopped)
        && state
            .jobs
            .iter()
            .all(|j| j.state.terminal() && !j.cleanup_pending)
}
pub fn clear(runtime: &Runtime) -> Result<(), String> {
    runtime.store.update(|state| {
        if !empty(state) {
            return Err("orphaned media state changed".into());
        }
        state.service = Service::default();
        Ok(())
    })
}
