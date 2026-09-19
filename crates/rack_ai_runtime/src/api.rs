use crate::{
    admission::Admission,
    control::{Control, ControlContext},
    service::Service,
    types::*,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Discover,
    Reserve {
        request: crate::reservation::Reserve,
    },
    RefreshReservation {
        reservation_id: String,
    },
    InspectReservation {
        reservation_id: String,
    },
    ReleaseReservation {
        reservation_id: String,
    },
    CancelReservation {
        reservation_id: String,
    },
    SubmitWork {
        request: crate::work_payload::Work,
    },
    InspectWork {
        work_id: String,
    },
    CancelWork {
        work_id: String,
    },
    Acquire {
        request: Acquire,
    },
    Inspect {
        reservation_id: String,
    },
    Control {
        reservation_id: String,
        request: Control,
    },
    Infer {
        request: Box<Inference>,
    },
    Result {
        invocation_id: String,
    },
    Cancel {
        invocation_id: String,
    },
    Reconcile {
        invocation_id: String,
    },
}
pub fn router(service: Arc<Service>) -> Router {
    Router::new()
        .route("/runtime/v1", post(handle))
        .route("/runtime/v1/contract", get(crate::contract::handle))
        .route(
            "/runtime/v1/voices/register",
            post(crate::voice_registration::handle).layer(DefaultBodyLimit::max(
                crate::voice_registration::MAX_FORM_BYTES,
            )),
        )
        .route(
            "/scoped/{id}/{generation}/v1/{*path}",
            post(crate::scoped_gateway::handle),
        )
        .route(
            "/scoped/{id}/{generation}/v1/scopes/{namespace}",
            post(crate::workspace_scope_api::handle),
        )
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(service)
}
async fn handle(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    request: Result<Json<Request>, axum::extract::rejection::JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let source = match authenticate(&service.config, &headers) {
        Ok(source) => source,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"schema": VERSION, "error":"unauthorized"})),
            );
        }
    };
    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            return (
                error.status(),
                Json(
                    json!({"schema":VERSION,"error":"invalid_request","detail":error.body_text()}),
                ),
            );
        }
    };
    let slots = if matches!(
        request,
        Request::Acquire { .. }
            | Request::Infer { .. }
            | Request::Reserve { .. }
            | Request::RefreshReservation { .. }
            | Request::SubmitWork { .. }
    ) {
        &service.admission_slots
    } else {
        &service.control_slots
    };
    let Ok(_permit) = slots.clone().try_acquire_owned() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"schema":VERSION,"error":"capacity_api_workers"})),
        );
    };
    let result = tokio::task::spawn_blocking(move || execute(&service, (&source, request))).await;
    match result {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)),
        Ok(Err(error)) => {
            let status = match error.as_str() {
                "capacity_pending_global"
                | "capacity_pending_reservation"
                | "capacity_reservation_call_history"
                | "capacity_active_evidence" => StatusCode::TOO_MANY_REQUESTS,
                "not_found" => StatusCode::NOT_FOUND,
                "source_spoofing" | "qualification_mode_denied" => StatusCode::FORBIDDEN,
                "identity_conflict"
                | "stale_generation"
                | "stale_generation_or_profile"
                | "reservation_not_dispatchable"
                | "reservation_preempting"
                | "reservation_preempted"
                | "reservation_terminal" => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            (status, Json(json!({"schema": VERSION, "error":error})))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"schema":VERSION,"error":"receiver_failure"})),
        ),
    }
}
fn execute(service: &Service, call: (&crate::config::Source, Request)) -> Result<Value, String> {
    let (source, request) = call;
    let reservation_action = if matches!(&request, Request::CancelReservation { .. }) {
        crate::control::Action::Cancel
    } else {
        crate::control::Action::Release
    };
    if matches!(&request, Request::Infer { request } if request.work.is_some()) {
        return Err("use_submit_work".into());
    }
    if matches!(&request, Request::Infer { request }
        if request.payload.as_ref().is_some_and(|p| p.protocol == crate::protocol::Protocol::Speech))
    {
        return Err("use_scoped_speech_gateway".into());
    }
    let value = match request {
        Request::Discover => {
            json!({"tags": service.config.profiles.iter().map(|p| json!({
            "tag":p.tag,"version":p.version,"qualified":p.qualified,"capabilities":p.capabilities,"context_tokens":p.context_tokens
        })).collect::<Vec<_>>(), "priorities":["low","medium","high","paramount"],"default_priority":"low"})
        }
        Request::Reserve { request } => {
            (crate::reservation_admission::ReservationAdmission { service, source })
                .reserve(request)?
        }
        Request::RefreshReservation { reservation_id } => {
            crate::reservation_refresh::refresh(service, (&source.source, &reservation_id))?
        }
        Request::InspectReservation { reservation_id } => {
            crate::reservation_view::inspect(service, (&source.source, &reservation_id))?
        }
        Request::ReleaseReservation { reservation_id }
        | Request::CancelReservation { reservation_id } => {
            crate::control::reservation_control(
                service,
                (&source.source, &reservation_id, reservation_action),
            )?;
            crate::reservation_view::inspect(service, (&source.source, &reservation_id))?
        }
        Request::SubmitWork { request } => {
            let id = request.work_id.clone();
            (crate::work::WorkSubmission { service }).submit((&source.source, request))?;
            crate::work::inspect(service, (&source.source, &id))?
        }
        Request::InspectWork { work_id } => {
            crate::work::inspect(service, (&source.source, &work_id))?
        }
        Request::CancelWork { work_id } => {
            crate::work::cancel(service, (&source.source, &work_id))?
        }
        Request::Acquire { request } => public(Admission { service, source }.acquire(request)?)?,
        Request::Inspect { reservation_id } => {
            public(service.inspect(&source.source, &reservation_id)?)?
        }
        Request::Control {
            reservation_id,
            request,
        } => public(
            (crate::control::ReservationControl { service }).control(ControlContext {
                owner: &source.source,
                id: &reservation_id,
                request,
            })?,
        )?,
        Request::Infer { request } => serde_json::to_value(
            (crate::inference::Submission { service }).submit(&source.source, *request)?,
        )
        .map_err(|e| e.to_string())?,
        Request::Result { invocation_id } | Request::Reconcile { invocation_id } => {
            serde_json::to_value(service.result(&source.source, &invocation_id)?)
                .map_err(|e| e.to_string())?
        }
        Request::Cancel { invocation_id } => serde_json::to_value(
            (crate::control::ReservationControl { service })
                .cancel_invocation(&source.source, &invocation_id)?,
        )
        .map_err(|e| e.to_string())?,
    };
    Ok(json!({"schema":VERSION, "result":value}))
}
pub(crate) fn public(d: Demand) -> Result<Value, String> {
    let mut value = serde_json::to_value(&d).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().ok_or("invalid_public_record")?;
    object.remove("profile");
    object.remove("reserve_result");
    object.remove("reservation_closed");
    object.remove("access_key");
    object.remove("backend_activation");
    if d.profile.backend != crate::config::Backend::Comfyui {
        object.insert(
            "gateway_path".into(),
            json!(format!("/scoped/{}/{}/v1", d.id, d.access_key)),
        );
    } else if d.profile.native_media() {
        object.insert("access".into(), crate::native_description::describe(&d)?);
    } else {
        object.insert(
            "access".into(),
            json!({"kind":"managed_image","jobs_path":"/api/media/v1/jobs"}),
        );
    }
    if !d.profile.native_media() {
        object.insert("model".into(), json!(d.profile.model));
    }
    object.insert("profile_version".into(), json!(d.profile.version));
    object.insert("resources".into(), json!(d.profile.resources));
    Ok(value)
}

pub(crate) fn authenticate(
    config: &crate::config::Config,
    headers: &HeaderMap,
) -> Result<crate::config::Source, String> {
    let token = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    config.authenticate(token)
}
