use crate::{runtime::Runtime, types::*};
pub struct ImageCancellation<'a> {
    pub runtime: &'a Runtime,
}
impl ImageCancellation<'_> {
    pub fn cancel(&self, job: &Job) -> Result<(), String> {
        let r = self.runtime;
        r.store.update(|s| {
            let j = s
                .jobs
                .iter_mut()
                .find(|j| j.id == job.id)
                .ok_or("job missing")?;
            if !j.cancel_requested {
                j.state = JobState::Failed;
                j.error = Some("job deadline exceeded".into());
            }
            j.updated_at = now();
            Ok(())
        })?;
        r.backend.cancel(job)?;
        if !r.backend.queue()?.contains(&job.prompt_id) {
            r.store.update(|s| {
                let j = s
                    .jobs
                    .iter_mut()
                    .find(|j| j.id == job.id)
                    .ok_or("job missing")?;
                j.cleanup_pending = false;
                s.service.last_busy = now();
                Ok(())
            })?;
        }
        Ok(())
    }
}
