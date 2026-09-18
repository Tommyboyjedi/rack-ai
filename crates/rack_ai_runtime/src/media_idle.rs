use crate::{
    service::{Service, active},
    types::*,
};
pub struct MediaIdle<'a> {
    pub service: &'a Service,
}
impl MediaIdle<'_> {
    pub fn observe(&self, d: &Demand) -> Result<(), String> {
        let adapter = crate::media::MediaAdapter {
            config: &self.service.config,
        };
        let runtime = adapter.runtime(d)?;
        let service = runtime.store.read()?.service;
        if service.activation != d.generation {
            return Err("media_activation_mismatch".into());
        }
        let idle = rack_ai_media::idle::IdleCheck { runtime: &runtime };
        self.service.authority.update(|s| {
            // Keep the shared activity decision atomic with inference admission.
            // Both native probes are bounded and use only the backend queue barrier.
            {
                let saved = crate::transition::current(s, d)?;
                if saved.state != DemandState::Ready || !active(saved) {
                    return Ok(());
                }
                let activity = idle.activity(&service)?;
                saved.last_activity_at = Some(
                    saved
                        .last_activity_at
                        .unwrap_or(saved.created)
                        .max(activity.last_activity_at),
                );
            }
            let seconds = self.service.config.idle_timeout_seconds;
            if crate::idle::group_active(s, (d, seconds, now())) {
                return Ok(());
            }
            let value = idle.observe(&service, seconds)?;
            let saved = crate::transition::current(s, d)?;
            saved.last_activity_at = Some(
                saved
                    .last_activity_at
                    .unwrap_or(saved.created)
                    .max(value.last_activity_at),
            );
            if value.idle_closed {
                crate::idle::expire(saved, now());
            }
            Ok(())
        })
    }
}
