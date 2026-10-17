//! Enforce frozen input limits before a durably accepted invocation generates.
use super::{Report, access, backend, payload};
use crate::{
    service::Service,
    types::{Demand, Invocation},
};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) struct RunningInput<'a> {
    pub demand: &'a Demand,
    pub invocation: &'a Invocation,
    pub body: &'a Value,
}
pub(crate) fn admit_running(
    service: &Service,
    input: RunningInput<'_>,
) -> Result<Duration, String> {
    admit(service, input).map_err(|error| format!("input_admission_{error}"))
}
fn admit(service: &Service, input: RunningInput<'_>) -> Result<Duration, String> {
    let demand = input.demand;
    // Dispatch froze ownership/generation and marked this call Running.
    // A priority drain may close new access while this accepted call completes.
    let payload = payload(demand, input.body.clone())?;
    access::text_only(&payload)?;
    let requested = payload.validate(demand)?;
    let _guard = rack_ai_infrastructure::endpoint_fence::managed_read_guard(
        &service.config.authority_root,
        &demand.profile.endpoint,
    )?;
    let process = demand.process.as_ref().ok_or("backend_identity_unproven")?;
    crate::process::endpoint_owned(process, &demand.profile)
        .map_err(|_| "backend_identity_unproven")?;
    let tokens = backend::count_bounded(
        service,
        backend::Counting {
            demand,
            payload: &payload,
            timeout: remaining(input.invocation)?,
        },
    )?;
    crate::process::endpoint_owned(process, &demand.profile)
        .map_err(|_| "backend_identity_unproven")?;
    if !Report::new(demand, (requested, tokens)).fits {
        return Err("limits_exceeded".into());
    }
    remaining(input.invocation)
}
fn remaining(invocation: &Invocation) -> Result<Duration, String> {
    let deadline = invocation
        .execution_deadline
        .ok_or("deadline_unavailable")?;
    (UNIX_EPOCH + Duration::from_secs(deadline))
        .duration_since(SystemTime::now())
        .ok()
        .filter(|value| !value.is_zero())
        .ok_or_else(|| "deadline_exceeded".into())
}
