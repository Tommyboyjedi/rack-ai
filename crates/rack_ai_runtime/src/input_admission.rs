//! Uniform invoke-time admission. Exact counters remain preferred; qualified
//! profiles without one use a conservative upper bound and never claim an exact count.
use crate::{
    service::Service,
    types::{Demand, Invocation},
};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STATIC_TEMPLATE_TOKEN_ALLOWANCE: u64 = 4096;

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
    conservative(&input).map_err(|error| format!("input_admission_{error}"))?;
    remaining(input.invocation).map_err(|error| format!("input_admission_{error}"))
}

fn conservative(input: &AdmissionInput<'_>) -> Result<(), String> {
    let profile = &input.demand.profile;
    let input_bound = conservative_upper_bound(input.body)?;
    let output = u64::from(input.invocation.request.max_tokens);
    if input_bound > u64::from(profile.effective_max_input_tokens())
        || input_bound + output + u64::from(profile.context_reserve_tokens)
            > u64::from(profile.context_tokens)
    {
        return Err("limits_exceeded".into());
    }
    Ok(())
}

fn conservative_upper_bound(body: &Value) -> Result<u64, String> {
    let bytes = serde_json::to_vec(body)
        .map_err(|_| "invalid_protocol_body")?
        .len();
    u64::try_from(bytes)
        .map_err(|_| "limits_exceeded".to_string())?
        .checked_add(STATIC_TEMPLATE_TOKEN_ALLOWANCE)
        .ok_or_else(|| "limits_exceeded".into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn static_admission_is_explicitly_conservative_not_an_exact_count() {
        let small = conservative_upper_bound(&json!({"messages":[{"content":"hello"}]})).unwrap();
        assert!(small >= STATIC_TEMPLATE_TOKEN_ALLOWANCE + 5);
        let large =
            conservative_upper_bound(&json!({"messages":[{"content":"x".repeat(1000)}]})).unwrap();
        assert!(large > small);
    }
}
