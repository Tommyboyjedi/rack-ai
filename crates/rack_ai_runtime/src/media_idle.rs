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
        let value = (rack_ai_media::idle::IdleCheck { runtime: &runtime })
            .observe(&service, self.service.config.idle_timeout_seconds)?;
        self.service.authority.update(|s| {
            let saved = crate::transition::current(s, d)?;
            if saved.state != DemandState::Ready || !active(saved) {
                return Ok(());
            }
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
