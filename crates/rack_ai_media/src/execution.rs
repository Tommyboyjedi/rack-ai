use crate::{artifacts, runtime::Runtime, types::*};
pub struct ImageExecution<'a> {
    pub runtime: &'a Runtime,
}
impl ImageExecution<'_> {
    pub fn tick(&self) -> Result<(), String> {
        let r = self.runtime;
        let state = r.store.read()?;
        if state.service.state != ServiceState::Ready || state.service.mode != Mode::Managed {
            return Ok(());
        }
        if let Some(job) = state.jobs.iter().find(|j| j.cleanup_pending) {
            return self.poll(job);
        }
        let selected = state
            .jobs
            .iter()
            .filter(|j| !j.state.terminal())
            .max_by_key(|j| (j.request.priority, std::cmp::Reverse(j.created_at)));
        if let Some(job) = selected {
            crate::dispatch::ImageDispatch { runtime: r }.dispatch(job)?;
        }
        Ok(())
    }

    fn poll(&self, job: &Job) -> Result<(), String> {
        let r = self.runtime;
        let current = r.store.read()?;
        if job.activation.as_ref() != Some(&current.service.activation) {
            return Err("job activation lost".into());
        }
        if job.cancel_requested || now() > job.deadline {
            return crate::cancellation::ImageCancellation { runtime: r }.cancel(job);
        }
        let queue = r.backend.queue()?;
        let history = r.backend.history(job)?;
        if let Some(terminal) = history.get(&job.prompt_id) {
            if queue.contains(&job.prompt_id) {
                return Ok(());
            }
            let result = artifacts::collect(&r.config, job, terminal);
            return r.store.update(|s| {
                let j = s
                    .jobs
                    .iter_mut()
                    .find(|j| j.id == job.id)
                    .ok_or("job missing")?;
                if !j.cancel_requested {
                    match result {
                        Ok(a) => {
                            j.artifacts = a;
                            j.state = JobState::Completed;
                            j.error = None;
                        }
                        Err(e) => {
                            j.state = JobState::Failed;
                            j.error = Some(e);
                        }
                    }
                }
                j.cleanup_pending = false;
                j.updated_at = now();
                s.service.last_busy = now();
                Ok(())
            });
        }
        if !queue.contains(&job.prompt_id) {
            return r.store.update(|s| {
                let j = s
                    .jobs
                    .iter_mut()
                    .find(|j| j.id == job.id)
                    .ok_or("job missing")?;
                if !j.cancel_requested {
                    j.state = JobState::Interrupted;
                    j.error = Some("submission outcome unknown; no redispatch permitted".into());
                }
                j.cleanup_pending = false;
                j.updated_at = now();
                Ok(())
            });
        }
        Ok(())
    }
}
