use crate::{api::authenticate, service::Service, types::VERSION};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};
use std::sync::{Arc, LazyLock};

static CONTRACT: LazyLock<Value> = LazyLock::new(|| {
    let request: Value =
        serde_json::from_str(include_str!("../../../config/runtime/request.schema.json"))
            .expect("embedded runtime request schema must be valid JSON");
    let response: Value =
        serde_json::from_str(include_str!("../../../config/runtime/response.schema.json"))
            .expect("embedded runtime response schema must be valid JSON");
    json!({
        "schema": "rack-ai/runtime-contract/v1",
        "contract_version": "1.0.0",
        "documentation": include_str!("../../../docs/reservation-work.md"),
        "request_schema": request,
        "response_schema": response,
    })
});

pub(crate) async fn handle(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if authenticate(&service.config, &headers).is_err() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"schema": VERSION, "error": "unauthorized"})),
        );
    }
    (StatusCode::OK, Json(CONTRACT.clone()))
}
