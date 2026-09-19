//! The native enqueue barrier decides inactivity against the actual backend queue.
use crate::{backend::GateStatus, runtime::Runtime, types::*};
use serde::Deserialize;
pub const IDLE_TIMEOUT: &str = "idle_timeout";
pub const fn default_seconds() -> u64 {
    30 * 60
}
#[derive(Deserialize)]
pub struct Observation {
    #[serde(flatten)]
    pub gate: GateStatus,
    pub last_activity_at: u64,
    pub busy: bool,
    pub idle_closed: bool,
}
pub struct IdleCheck<'a> {
    pub runtime: &'a Runtime,
}
impl IdleCheck<'_> {
    pub fn observe(&self, service: &Service, seconds: u64) -> Result<Observation, String> {
        let value: Observation = self.runtime.backend.post(
            "/rack-gate/idle",
            &serde_json::json!({"activation":service.activation,"idle_timeout_seconds":seconds}),
        )?;
        self.validate(service, value)
    }
    pub fn activity(&self, service: &Service) -> Result<Observation, String> {
        let value = self
            .runtime
            .backend
            .post("/rack-gate/status", &serde_json::json!({}))?;
        self.validate(service, value)
    }
    fn validate(&self, service: &Service, value: Observation) -> Result<Observation, String> {
        if value.gate.activation != service.activation
            || service.invocation.as_ref() != Some(&value.gate.invocation)
            || value.gate.protocol != "rack-gate/v1"
            || service
                .generation
                .as_ref()
                .is_some_and(|g| g.pid != value.gate.pid)
            || (value.idle_closed && (value.busy || value.gate.mode != Mode::Closed))
            || value.last_activity_at > now()
        {
            return Err("idle_gate_identity_or_activity_unproven".into());
        }
        Ok(value)
    }
    pub fn manual(&self, service: &Service) -> Result<bool, String> {
        let value = self.observe(service, self.runtime.config.reservation_idle_seconds)?;
        self.runtime.store.update(|s| {
            if s.service.activation != service.activation || s.service.state != ServiceState::Ready
            {
                return Err("idle_media_generation_changed".into());
            }
            s.service.last_busy = s.service.last_busy.max(value.last_activity_at);
            if value.idle_closed {
                for session in &mut s.sessions {
                    if Some(&session.id) == service.session.as_ref() {
                        session.release_requested = true;
                        session.terminal_reason = Some(IDLE_TIMEOUT.into());
                    }
                }
                s.service.state = ServiceState::Draining;
                s.service.since = now();
                s.service.error = Some(IDLE_TIMEOUT.into());
            }
            Ok(value.idle_closed)
        })
    }
}
