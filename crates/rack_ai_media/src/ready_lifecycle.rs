use crate::{runtime::Runtime, types::*};
pub struct ReadyLifecycle<'a> {
    pub runtime: &'a Runtime,
}
impl ReadyLifecycle<'_> {
    pub fn tick(&self, state: &MediaState) -> Result<(), String> {
        let r = self.runtime;
        let s = &state.service;
        crate::lifecycle::Lifecycle { runtime: r }.verify(s)?;
        crate::lifecycle::Lifecycle { runtime: r }.authority(s)?;
        let shared = s
            .lease
            .as_ref()
            .map(|handle| {
                (rack_ai_infrastructure::managed_lease::ManagedLease {
                    resources: &r.reservations,
                })
                .verify(handle, false)
            })
            .transpose()?
            .unwrap_or(false);
        let expired = !shared
            && state.sessions.iter().any(|x| {
                Some(&x.id) == s.session.as_ref() && now() > x.created_at + r.config.session_seconds
            });
        let finish = if s.mode == Mode::Interactive {
            !state
                .sessions
                .iter()
                .any(|x| Some(&x.id) == s.session.as_ref() && !x.release_requested)
                || expired
        } else {
            !state
                .jobs
                .iter()
                .any(|j| !j.state.terminal() || j.cleanup_pending)
                && now() >= s.last_busy + r.config.idle_seconds
        };
        if finish && !(shared && s.mode == Mode::Managed) {
            crate::lifecycle::Lifecycle { runtime: r }.transition(ServiceState::Draining)?;
        }
        Ok(())
    }
}
