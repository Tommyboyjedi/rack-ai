use crate::{
    api::ApiRequest,
    resource_command::{Report, SCHEMA},
    web_state::WebState,
};
use axum::http::StatusCode;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryRequest {
    schema: String,
    confirm: String,
}
pub fn handle(
    app: &WebState,
    input: &ApiRequest,
) -> Result<(StatusCode, serde_json::Value), String> {
    if !input.principal.operator {
        return Err("forbidden: operator access required".into());
    }
    let apply = input.path == "resources/recover";
    if apply {
        let request: RecoveryRequest = serde_json::from_slice(&input.bytes)
            .map_err(|_| "validation: invalid recovery request")?;
        if request.schema != SCHEMA || request.confirm != "recover_stuck_resources" {
            return Err("validation: confirm resource recovery".into());
        }
    }
    let command = app
        .resource_command
        .as_ref()
        .ok_or("unavailable: resource recovery is not configured")?;
    let _permit = app
        .resource_control
        .try_lock()
        .map_err(|_| "conflict: recovery already running")?;
    let report: Report = command
        .execute(apply)
        .map_err(|_| "unavailable: resource recovery could not complete; refresh status")?;
    if apply {
        let text = serde_json::to_string(&serde_json::json!({
            "at":crate::types::now(),"operator":input.principal.id,"report":report
        }))
        .map_err(|e| e.to_string())?;
        rack_ai_application::durable_file::atomic_write(
            &app.store.root.join("resource-recovery-last.json"),
            &text,
        )
        .map_err(
            |_| "unavailable: recovery ran but its result could not be recorded; refresh status",
        )?;
        eprintln!(
            "operator resource recovery requested by {}",
            input.principal.id
        );
    }
    Ok((
        StatusCode::OK,
        serde_json::to_value(report).map_err(|e| e.to_string())?,
    ))
}
