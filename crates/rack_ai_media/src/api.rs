use crate::{
    admission::Admission,
    config::Principal,
    types::*,
    web_state::{WebState, blocking},
};
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
pub async fn handle(State(app): State<WebState>, request: Request) -> Response {
    let Some(principal) = request.extensions().get::<Principal>().cloned() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let method = request.method().clone();
    let path = request
        .uri()
        .path()
        .trim_start_matches("/api/media/v1/")
        .to_string();
    let bytes =
        match axum::body::to_bytes(request.into_body(), crate::limits::JSON_BODY_BYTES).await {
            Ok(v) => v,
            Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        };
    let result = blocking(move || {
        dispatch(
            &app,
            ApiRequest {
                principal,
                method,
                path,
                bytes: bytes.to_vec(),
            },
        )
    })
    .await;
    match result {
        Ok((status, value)) => {
            let location = value
                .get("location")
                .and_then(|v| v.as_str())
                .and_then(|v| v.parse().ok());
            let mut response = (status, Json(value)).into_response();
            if status == StatusCode::ACCEPTED {
                if let Some(location) = location {
                    response
                        .headers_mut()
                        .insert(axum::http::header::LOCATION, location);
                }
            }
            response
        }
        Err(e) => error(&e),
    }
}
pub struct ApiRequest {
    pub principal: Principal,
    pub method: axum::http::Method,
    pub path: String,
    pub bytes: Vec<u8>,
}
fn dispatch(app: &WebState, input: ApiRequest) -> Result<(StatusCode, serde_json::Value), String> {
    use axum::http::Method;
    let p = &input.principal;
    match (input.method.clone(), input.path.as_str()) {
        (Method::GET, "status") => {
            let s = app.store.read()?;
            let session = s
                .sessions
                .iter()
                .find(|x| x.owner == p.id && !x.stopped)
                .map(|x| x.id.clone());
            Ok((
                StatusCode::OK,
                serde_json::json!({"schema":VERSION,"state":s.service.state,"mode":s.service.mode,
                "heartbeat":s.service.heartbeat,"session_id":session,"message":s.service.error}),
            ))
        }
        (Method::GET, "profiles") => Ok((
            StatusCode::OK,
            serde_json::json!({"schema":VERSION,"profiles":
            if app.config.profile.available{vec![serde_json::json!({"id":app.config.profile.id,"version":app.config.profile.version,
                "operation":"text_to_image","width":{"min":64,"max":1024,"multiple":64},"height":{"min":64,"max":1024,"multiple":64},
                "steps":{"min":1,"max":50},"seed_max":i64::MAX,"prompt_max":4096})]}else{vec![]}}),
        )),
        (Method::POST, "jobs") => {
            let request: JobRequest =
                serde_json::from_slice(&input.bytes).map_err(|_| "validation: invalid job")?;
            let job = Admission {
                config: &app.config,
                store: &app.store,
            }
            .job(p, request)?;
            Ok((StatusCode::ACCEPTED, job_view(&job)))
        }
        (Method::POST, "sessions") => {
            let request: SessionRequest =
                serde_json::from_slice(&input.bytes).map_err(|_| "validation: invalid session")?;
            let session = Admission {
                config: &app.config,
                store: &app.store,
            }
            .session(p, request)?;
            Ok((StatusCode::ACCEPTED, session_view(app, &session)?))
        }
        _ => crate::api_items::dispatch(app, &input),
    }
}
pub fn job_view(job: &Job) -> serde_json::Value {
    serde_json::json!({"schema":VERSION,"id":job.id,"location":format!("/api/media/v1/jobs/{}",job.id),
        "work_id":job.request.work_id,"submission_id":job.request.submission_id,"state":job.state,
        "profile":job.request.profile,"profile_version":job.request.profile_version,"parameters":job.request.parameters,
        "prompt_id":job.prompt_id,"activation":job.activation,"workflow_sha256":job.workflow_sha256,
        "model_identity":job.model_identity,"created_at":job.created_at,"updated_at":job.updated_at,
        "cancel_requested":job.cancel_requested,"artifacts":job.artifacts,"error":job.error})
}
pub fn session_view(app: &WebState, session: &Session) -> Result<serde_json::Value, String> {
    let state = app.store.read()?;
    let active = state.service.session.as_ref() == Some(&session.id);
    Ok(
        serde_json::json!({"schema":VERSION,"id":session.id,"location":format!("/api/media/v1/sessions/{}",session.id),
        "state":if session.stopped{"stopped".into()}else if active{serde_json::to_value(state.service.state).map_err(|e|e.to_string())?}
        else{"waiting".into()},"release_requested":session.release_requested,
        "access_url":if active && state.service.state==ServiceState::Ready && !session.release_requested{Some(&app.config.native_origin)}else{None}}),
    )
}
pub fn error(message: &str) -> Response {
    let code = message.split(':').next().unwrap_or("internal");
    let status = match code {
        "validation" => StatusCode::UNPROCESSABLE_ENTITY,
        "unsupported" => StatusCode::UNPROCESSABLE_ENTITY,
        "conflict" => StatusCode::CONFLICT,
        "forbidden" => StatusCode::FORBIDDEN,
        "not_found" => StatusCode::NOT_FOUND,
        "unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::json!({"schema":VERSION,"error":{"code":code,"message":message}})),
    )
        .into_response()
}
