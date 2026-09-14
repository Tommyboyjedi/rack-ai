use crate::{
    config::{Config, Principal},
    profile,
    store::Store,
    types::*,
};
pub struct Admission<'a> {
    pub config: &'a Config,
    pub store: &'a Store,
}
impl Admission<'_> {
    pub fn job(&self, principal: &Principal, request: JobRequest) -> Result<Job, String> {
        JobAdmission {
            config: self.config,
            store: self.store,
        }
        .job(principal, request)
    }
    pub fn session(
        &self,
        principal: &Principal,
        request: SessionRequest,
    ) -> Result<Session, String> {
        SessionAdmission { store: self.store }.session(principal, request)
    }
}
struct JobAdmission<'a> {
    config: &'a Config,
    store: &'a Store,
}
impl JobAdmission<'_> {
    pub fn job(&self, principal: &Principal, request: JobRequest) -> Result<Job, String> {
        if request.priority > principal.ceiling {
            return Err("forbidden: priority exceeds source ceiling".into());
        }
        self.store.update(|state| {
            if let Some(job) = state.jobs.iter().find(|j| {
                j.owner == principal.id
                    && (j.request.idempotency_key == request.idempotency_key
                        || j.request.submission_id == request.submission_id)
            }) {
                return if job.request == request {
                    Ok(job.clone())
                } else {
                    Err("conflict: identity payload changed".into())
                };
            }
            crate::shared_job::SharedJob {
                config: self.config,
                store: self.store,
            }
            .authorize(principal, &request)?;
            profile::validate(&request, &self.config.profile)?;
            if state.jobs.iter().filter(|j| !j.state.terminal()).count()
                >= crate::limits::ACTIVE_JOBS
                || state.jobs.len() >= crate::limits::RETAINED_RECORDS
            {
                return Err("unavailable: job capacity reached".into());
            }
            let id = identity();
            let workflow = profile::workflow(&request, &self.config.profile, &id)?;
            let deadline = now() + request.timeout_seconds;
            let job = Job {
                id,
                owner: principal.id.clone(),
                request,
                state: JobState::Waiting,
                created_at: now(),
                updated_at: now(),
                deadline,
                cleanup_pending: false,
                cancel_requested: false,
                prompt_id: identity(),
                activation: None,
                workflow_sha256: profile::workflow_hash(&workflow)?,
                workflow,
                model_identity: self.config.profile.checkpoint_sha256.clone(),
                dispatched_at: None,
                artifacts: vec![],
                error: None,
            };
            state.jobs.push(job.clone());
            Ok(job)
        })
    }
}
struct SessionAdmission<'a> {
    store: &'a Store,
}
impl SessionAdmission<'_> {
    pub fn session(
        &self,
        principal: &Principal,
        request: SessionRequest,
    ) -> Result<Session, String> {
        if !principal.operator {
            return Err("forbidden: interactive access requires operator".into());
        }
        if request.schema != VERSION
            || request.idempotency_key.is_empty()
            || request.idempotency_key.len() > 128
        {
            return Err("validation: invalid session request".into());
        }
        self.store.update(|state| {
            if let Some(s) = state.sessions.iter().find(|s| {
                s.owner == principal.id && s.request.idempotency_key == request.idempotency_key
            }) {
                return if s.request == request {
                    Ok(s.clone())
                } else {
                    Err("conflict: session payload changed".into())
                };
            }
            if let Some(s) = state
                .sessions
                .iter()
                .find(|s| s.owner == principal.id && !s.stopped)
            {
                return Ok(s.clone());
            }
            if state.sessions.len() >= crate::limits::RETAINED_RECORDS {
                return Err("unavailable: session retention full".into());
            }
            let session = Session {
                id: identity(),
                owner: principal.id.clone(),
                request,
                created_at: now(),
                release_requested: false,
                stopped: false,
            };
            state.sessions.push(session.clone());
            Ok(session)
        })
    }
}
