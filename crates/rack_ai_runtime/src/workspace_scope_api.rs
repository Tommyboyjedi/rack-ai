use crate::{
    service::Service,
    workspace_scope::{ScopeAccess, ScopeControl, ScopeController},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;

pub async fn handle(
    State(service): State<Arc<Service>>,
    Path((reservation, capability, namespace)): Path<(String, String, String)>,
    Json(operation): Json<ScopeControl>,
) -> axum::response::Response {
    let slots = if matches!(operation, ScopeControl::Open { .. }) {
        &service.admission_slots
    } else {
        &service.control_slots
    };
    let Ok(_permit) = slots.clone().try_acquire_owned() else {
        return crate::scoped_gateway::failure("capacity_scope_controls".into());
    };
    let result = tokio::task::spawn_blocking(move || {
        ScopeController { service: &service }.control(
            ScopeAccess {
                reservation,
                capability,
                namespace,
            },
            operation,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) if error.starts_with("capacity_") => crate::scoped_gateway::failure(error),
        Ok(Err(error)) => crate::scoped_gateway::failure(format!(
            "workspace_scope_persistence_or_control_failed:{error}"
        )),
        Err(error) => {
            crate::scoped_gateway::failure(format!("workspace_scope_receiver_failure:{error}"))
        }
    }
}
