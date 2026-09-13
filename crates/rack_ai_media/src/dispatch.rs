use crate::{runtime::Runtime, types::*};
pub struct ImageDispatch<'a> {
    pub runtime: &'a Runtime,
}
impl ImageDispatch<'_> {
    pub fn dispatch(&self, job: &Job) -> Result<(), String> {
        let r = self.runtime;
        let prepared = r.store.update(|s| {
            let j = s
                .jobs
                .iter_mut()
                .find(|j| j.id == job.id)
                .ok_or("job missing")?;
            if j.cancel_requested || j.state.terminal() {
                return Ok(None);
            }
            if now() > j.deadline {
                j.state = JobState::Failed;
                j.error = Some("job deadline before dispatch".into());
                return Ok(None);
            }
            j.state = JobState::Dispatching;
            j.dispatched_at = Some(now());
            j.cleanup_pending = true;
            j.activation = Some(s.service.activation.clone());
            j.updated_at = now();
            s.service.last_busy = now();
            Ok(Some(j.clone()))
        })?;
        let Some(prepared) = prepared else {
            return Ok(());
        };
        let result = r.backend.submit(&prepared);
        r.store.update(|s| {
            let j = s
                .jobs
                .iter_mut()
                .find(|j| j.id == job.id)
                .ok_or("job missing")?;
            if !j.cancel_requested {
                j.state = if result.is_ok() {
                    JobState::Running
                } else {
                    JobState::SubmissionUncertain
                };
            }
            j.error = result.err();
            j.updated_at = now();
            Ok(())
        })
    }
}
