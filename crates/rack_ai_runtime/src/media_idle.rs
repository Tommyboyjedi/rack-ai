use crate::{
    service::{Service, active},
    types::*,
};
pub struct MediaIdle<'a> {
    pub service: &'a Service,
}
impl MediaIdle<'_> {
    pub fn observe(&self, d: &Demand) -> Result<(), String> {
        let should_probe = self.service.authority.read(|s| {
            let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
            if saved.generation != d.generation {
                return Err("stale_transition_callback".into());
            }
            Ok(saved.state == DemandState::Ready && active(saved))
        })?;
        if !should_probe {
            return Ok(());
        }
        let adapter = crate::media::MediaAdapter {
            config: &self.service.config,
        };
        let runtime = adapter.runtime(d)?;
        let service = runtime.store.read()?.service;
        if service.activation != d.generation {
            return Err("media_activation_mismatch".into());
        }
        let idle = rack_ai_media::idle::IdleCheck { runtime: &runtime };
        let activity = idle.activity(&service)?;
        let observed_activity_at = if activity.busy {
            now()
        } else {
            activity.last_activity_at
        };
        self.service.authority.update(|s| {
            // Keep the shared activity decision atomic with inference admission.
            // The native status probe is validated against the frozen media
            // identity before this lock; do not hold the global authority lock
            // while waiting on the ComfyUI HTTP gate.
            {
                let saved = crate::transition::current(s, d)?;
                if saved.state != DemandState::Ready || !active(saved) {
                    return Ok(());
                }
                saved.last_activity_at = Some(
                    saved
                        .last_activity_at
                        .unwrap_or(saved.created)
                        .max(observed_activity_at),
                );
            }
            let seconds = self.service.config.idle_timeout_seconds;
            crate::activity_retention::refresh(s, (seconds, now()))?;
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
