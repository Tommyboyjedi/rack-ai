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
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Discover,
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
        request: Inference,
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
    let token = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    let source = match service.config.authenticate(token) {
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
    let slots = if matches!(request, Request::Acquire { .. } | Request::Infer { .. }) {
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
                | "capacity_retained_evidence" => StatusCode::TOO_MANY_REQUESTS,
                "not_found" => StatusCode::NOT_FOUND,
                "source_spoofing" | "source_policy_denied" => StatusCode::FORBIDDEN,
                "identity_conflict"
                | "stale_generation"
                | "stale_generation_or_profile"
                | "reservation_not_dispatchable"
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
    let value = match request {
        Request::Discover => {
            json!({"tags": service.config.profiles.iter().filter(|p| source.tags.contains(&p.tag)).map(|p| json!({
            "tag":p.tag,"version":p.version,"qualified":p.qualified,"capabilities":p.capabilities,"context_tokens":p.context_tokens
        })).collect::<Vec<_>>(), "permitted_priorities": source.permitted,"default_priority":source.default,"maximum_priority":source.maximum})
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
            (crate::inference::Submission { service }).submit(&source.source, request)?,
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
fn public(d: Demand) -> Result<Value, String> {
    let mut value = serde_json::to_value(&d).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().ok_or("invalid_public_record")?;
    object.remove("profile");
    object.remove("access_key");
    object.insert(
        "gateway_path".into(),
        json!(format!("/scoped/{}/{}/v1", d.id, d.access_key)),
    );
    if !d.profile.native_media() {
        object.insert("model".into(), json!(d.profile.model));
    }
    object.insert("profile_version".into(), json!(d.profile.version));
    object.insert("resources".into(), json!(d.profile.resources));
    Ok(value)
}
