//! Only a newly validated, reservation-bound media job is GPU admission activity.
use crate::{backend::Backend, config::Config, shared_job::MediaReservation};
pub struct JobActivity<'a> {
    pub config: &'a Config,
}
impl JobActivity<'_> {
    pub fn admit(&self, binding: &MediaReservation) -> Result<(), String> {
        let backend = Backend::new(self.config)?;
        let observation: crate::idle::Observation = backend.post(
            "/rack-gate/admit-job",
            &serde_json::json!({"activation":binding.generation}),
        )?;
        if observation.gate.activation != binding.generation
            || observation.gate.mode != crate::types::Mode::Managed
            || observation.idle_closed
        {
            return Err("conflict: managed GPU admission closed".into());
        }
        Ok(())
    }
}
