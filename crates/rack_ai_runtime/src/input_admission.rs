//! Uniform invoke-time admission. Exact counters remain preferred; qualified
//! profiles without one use a conservative upper bound and never claim an exact count.
use crate::{
    service::Service,
    types::{Demand, Invocation},
};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct AdmissionInput<'a> {
    pub demand: &'a Demand,
    pub invocation: &'a Invocation,
    pub body: &'a Value,
}

pub fn admit(service: &Service, input: AdmissionInput<'_>) -> Result<Duration, String> {
    if input.demand.profile.enforce_input_limits {
        return crate::input_preflight::admit_running(
            service,
            crate::input_preflight::RunningInput {
                demand: input.demand,
                invocation: input.invocation,
                body: input.body,
            },
        );
    }
    remaining(input.invocation).map_err(|error| format!("input_admission_{error}"))
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
