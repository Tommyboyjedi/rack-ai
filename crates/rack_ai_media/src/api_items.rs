use crate::{
    api::{ApiRequest, job_view, session_view},
    types::*,
    web_state::WebState,
};
use axum::http::{Method, StatusCode};
pub fn dispatch(
    app: &WebState,
    input: &ApiRequest,
) -> Result<(StatusCode, serde_json::Value), String> {
    let parts: Vec<_> = input.path.split('/').collect();
    if parts.len() < 2 || uuid::Uuid::parse_str(parts[1]).is_err() {
        return Err("not_found: unknown resource".into());
    }
    match (input.method.clone(), parts.as_slice()) {
        (Method::GET, ["jobs", id]) => {
            let s = app.store.read()?;
            let j = s
                .jobs
                .iter()
                .find(|j| j.id == *id && j.owner == input.principal.id)
                .ok_or("not_found: job")?;
            Ok((StatusCode::OK, job_view(j)))
        }
        (Method::POST, ["jobs", id, "cancel"]) => {
            let job = app.store.update(|s| {
                let j = s
                    .jobs
                    .iter_mut()
                    .find(|j| j.id == *id && j.owner == input.principal.id)
                    .ok_or("not_found: job")?;
                if j.state == JobState::Completed {
                    return Err("conflict: job already completed".into());
                }
                j.cancel_requested = true;
                j.state = JobState::Cancelled;
                j.updated_at = now();
                Ok(j.clone())
            })?;
            Ok((StatusCode::ACCEPTED, job_view(&job)))
        }
        (Method::GET, ["sessions", id]) => {
            let s = app.store.read()?;
            let session = s
                .sessions
                .iter()
                .find(|s| s.id == *id && s.owner == input.principal.id)
                .ok_or("not_found: session")?;
            Ok((StatusCode::OK, session_view(app, session)?))
        }
        (Method::POST, ["sessions", id, "release"]) => {
            let session = app.store.update(|s| {
                let active = s.service.session.clone();
                let session = s
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == *id && s.owner == input.principal.id)
                    .ok_or("not_found: session")?;
                session.release_requested = true;
                if active.as_ref() != Some(&session.id) {
                    session.stopped = true;
                } else if s.service.state == ServiceState::Ready {
                    s.service.state = ServiceState::Draining;
                    s.service.since = now();
                }
                Ok(session.clone())
            })?;
            Ok((StatusCode::ACCEPTED, session_view(app, &session)?))
        }
        _ => Err("not_found: unknown route".into()),
    }
}
