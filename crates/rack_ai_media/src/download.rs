use crate::{
    artifacts,
    config::Principal,
    types::{JobState, digest},
    web_state::{WebState, blocking},
};
use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
pub async fn handle(State(app): State<WebState>, request: Request) -> Response {
    let Some(principal) = request.extensions().get::<Principal>().cloned() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let pieces: Vec<_> = request.uri().path().split('/').collect();
    if pieces.len() != 8
        || uuid::Uuid::parse_str(pieces[5]).is_err()
        || uuid::Uuid::parse_str(pieces[7]).is_err()
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let job_id = pieces[5].to_owned();
    let artifact_id = pieces[7].to_owned();
    let result = blocking(move || {
        let state = app.store.read()?;
        let job = state
            .jobs
            .iter()
            .find(|j| j.id == job_id && j.owner == principal.id && j.state == JobState::Completed)
            .ok_or("not_found: artifact")?;
        let artifact = job
            .artifacts
            .iter()
            .find(|a| a.id == artifact_id)
            .ok_or("not_found: artifact")?;
        let path = app
            .config
            .state_root
            .join("artifacts")
            .join(&job.id)
            .join(format!("{}.png", artifact.id));
        let bytes = artifacts::read_bounded(&path)?;
        if digest(&bytes) != artifact.sha256 || bytes.len() as u64 != artifact.bytes {
            return Err("artifact integrity failed".into());
        }
        Ok(bytes)
    })
    .await;
    match result {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=image.png",
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => crate::api::error(&e),
    }
}
