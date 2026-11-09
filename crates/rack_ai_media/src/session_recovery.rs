//! Explicit operator recovery of a physically absent native backend.
use crate::{api::ApiRequest, gpu::GpuProbe, runtime::Runtime, types::*, web_state::WebState};
use axum::http::StatusCode;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
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
    let request: Request =
        serde_json::from_slice(&input.bytes).map_err(|_| "validation: invalid recovery request")?;
    if request.schema != VERSION || request.confirm != "recover_comfyui" {
        return Err("validation: confirm ComfyUI recovery".into());
    }
    let _operation = crate::operation::lock(&app.store.root)?;
    let runtime = Runtime::new((*app.config).clone())?;
    let state = runtime.store.read()?;
    verify(&runtime, (&state, &input.principal))?;
    let observed = runtime.systemd.observe()?;
    if state.service.invocation.is_some()
        && !observed.invocation.is_empty()
        && state.service.invocation.as_ref() != Some(&observed.invocation)
    {
        return Err("conflict: failed backend invocation changed".into());
    }
    rack_ai_application::durable_file::atomic_write(
        &app.store.root.join("session-recovery-last.json"),
        &serde_json::json!({"at":now(),"operator":input.principal.id,"service":state.service})
            .to_string(),
    )?;
    if observed.active == "failed" {
        crate::command::run(
            "systemctl",
            &["--user", "reset-failed", &runtime.config.unit],
        )?;
    }
    if state.service.lease.is_some() {
        crate::supervisor::recover(&runtime)?;
    } else {
        runtime.store.update(|s| {
            s.service = Service::default();
            Ok(())
        })?;
    }
    Ok((
        StatusCode::ACCEPTED,
        serde_json::json!({"schema":VERSION,"state":"stopped"}),
    ))
}

fn verify(
    runtime: &Runtime,
    input: (&MediaState, &crate::config::Principal),
) -> Result<(), String> {
    let (state, principal) = input;
    let service = &state.service;
    if service.state != ServiceState::RecoveryRequired
        || service.mode == Mode::Managed
        || state
            .jobs
            .iter()
            .any(|j| !j.state.terminal() || j.cleanup_pending)
        || state
            .sessions
            .iter()
            .any(|s| !s.stopped && s.owner != principal.id)
    {
        return Err(
            "conflict: recovery requires your failed native session without managed work".into(),
        );
    }
    crate::unit_definition::UnitDefinition {
        config: &runtime.config,
    }
    .verify()
    .map_err(|e| format!("conflict: {e}"))?;
    let gpu = GpuProbe {
        config: &runtime.config,
    };
    gpu.placement()?;
    if !runtime.systemd.gone()? || !gpu.pids()?.is_empty() {
        return Err(
            "conflict: backend or GPU processes remain; recovery requires verified absence".into(),
        );
    }
    let resource = "gpu-4080-super".to_string();
    let claims =
        rack_ai_infrastructure::managed_authority::read_claims(runtime.reservations.root())?;
    if claims.contains_key(&resource) {
        return Err("conflict: resource belongs to the RackAI runtime".into());
    }
    if let Some(handle) = &service.lease {
        if handle.owner != format!("media:{}", service.activation)
            || handle.paths.keys().map(String::as_str).collect::<Vec<_>>() != [resource.as_str()]
        {
            return Err("conflict: native lease ownership is unproven".into());
        }
        runtime
            .reservations
            .verify(handle)
            .map_err(|e| format!("conflict: {e}"))?;
    } else if !runtime.reservations.blocked(&[resource])?.is_empty()
        || service.invocation.is_some()
        || service.generation.is_some()
        || !service.activation.is_empty()
    {
        return Err("conflict: identity-free recovery has unresolved ownership".into());
    }
    Ok(())
}
