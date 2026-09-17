//! Queue capacity is a reservation contract. Terminal evidence is compacted to
//! digests before it can consume active-call admission headroom.
use crate::types::*;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_pending: usize,
    pub max_pending_per_reservation: usize,
    pub max_calls_per_reservation: u64,
    pub max_dispatch_workers: usize,
    pub max_transition_workers: usize,
    pub max_gateway_waiters: usize,
    pub max_wait_seconds: u64,
    pub max_response_bytes: u64,
    /// Bound for active queue reservations plus compact control data.
    pub retention_admission_bytes: u64,
    /// Full terminal payloads are retained only within this bounded window;
    /// older terminal payloads retain a durable SHA-256 receipt instead.
    pub terminal_evidence_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pending: 32,
            max_pending_per_reservation: 16,
            max_calls_per_reservation: 256,
            max_dispatch_workers: 4,
            max_transition_workers: 8,
            max_gateway_waiters: 32,
            max_wait_seconds: 300,
            max_response_bytes: 256 * 1024,
            retention_admission_bytes: 30 * 1024 * 1024,
            terminal_evidence_bytes: 8 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_pending == 0
            || self.max_pending > 1024
            || self.max_pending_per_reservation == 0
            || self.max_pending_per_reservation > self.max_pending
            || self.max_calls_per_reservation < self.max_pending_per_reservation as u64
            || self.max_calls_per_reservation > 16384
            || self.max_dispatch_workers == 0
            || self.max_dispatch_workers > 64
            || self.max_transition_workers == 0
            || self.max_transition_workers > 64
            || self.max_gateway_waiters == 0
            || self.max_gateway_waiters > 128
            || self.max_wait_seconds == 0
            || self.max_wait_seconds > 86400
            || !(1024..=4 * 1024 * 1024).contains(&self.max_response_bytes)
            || !(256 * 1024..=30 * 1024 * 1024).contains(&self.retention_admission_bytes)
            || !(64 * 1024..=30 * 1024 * 1024).contains(&self.terminal_evidence_bytes)
        {
            return Err("invalid_runtime_capacity_limits".into());
        }
        Ok(())
    }

    pub fn pending(&self, s: &Document, reservation: &str) -> Result<(), String> {
        let pending: Vec<_> = s
            .data
            .invocations
            .values()
            .filter(|i| matches!(i.state, InvocationState::Queued | InvocationState::Running))
            .collect();
        if pending.len() >= self.max_pending {
            return Err("capacity_pending_global".into());
        }
        if pending
            .iter()
            .filter(|i| i.request.reservation_id == reservation)
            .count()
            >= self.max_pending_per_reservation
        {
            return Err("capacity_pending_reservation".into());
        }
        let demand = s
            .data
            .demands
            .get(reservation)
            .ok_or("missing_reservation")?;
        if demand.accepted_calls >= self.max_calls_per_reservation {
            return Err("capacity_reservation_call_history".into());
        }
        Ok(())
    }
}

/// Compact only terminal response bodies. Invocation identities, requests,
/// states, errors and result digests remain authoritative for idempotency and
/// reconciliation. Active queued/running work is never compacted.
pub fn retention(s: &mut Document, limits: &Limits) -> Result<(), String> {
    compact_terminal_payloads(
        s,
        limits
            .terminal_evidence_bytes
            .min(limits.retention_admission_bytes / 2),
    )?;
    let mut reserved = serde_json::to_vec(s).map_err(|e| e.to_string())?.len() as u64;
    for invocation in s.data.invocations.values() {
        if matches!(
            invocation.state,
            InvocationState::Queued | InvocationState::Running
        ) {
            // JSON escaping can expand raw protocol output by six; envelope and
            // terminal transition diagnostics have fixed bounded overhead.
            reserved = reserved.saturating_add(invocation.response_bytes.saturating_mul(6) + 16384);
        }
    }
    if reserved > limits.retention_admission_bytes {
        return Err("capacity_active_evidence".into());
    }
    Ok(())
}

fn compact_terminal_payloads(s: &mut Document, limit: u64) -> Result<(), String> {
    let mut entries = Vec::new();
    for (id, invocation) in &s.data.invocations {
        if !matches!(
            invocation.state,
            InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
        ) {
            let result = invocation
                .result
                .as_ref()
                .map(serialized_bytes)
                .transpose()?
                .unwrap_or(0);
            let late = invocation
                .late_result
                .as_ref()
                .map(serialized_bytes)
                .transpose()?
                .unwrap_or(0);
            entries.push((invocation.started.unwrap_or(0), id.clone(), result + late));
        }
    }
    let mut total = entries.iter().map(|(_, _, bytes)| *bytes).sum::<u64>();
    if total <= limit {
        return Ok(());
    }
    entries.sort_by_key(|(started, id, _)| (*started, id.clone()));
    for (_, id, _) in entries {
        if total <= limit {
            break;
        }
        let invocation = s
            .data
            .invocations
            .get_mut(&id)
            .ok_or("missing_invocation")?;
        if let Some(value) = invocation.result.take() {
            let bytes = serialized_bytes(&value)?;
            total = total.saturating_sub(bytes);
            invocation.result_digest = Some(digest(
                &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
            ));
        }
        if total <= limit {
            break;
        }
        if let Some(value) = invocation.late_result.take() {
            let bytes = serialized_bytes(&value)?;
            total = total.saturating_sub(bytes);
            invocation.late_result_digest = Some(digest(
                &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
            ));
        }
    }
    Ok(())
}

fn serialized_bytes(value: &serde_json::Value) -> Result<u64, String> {
    Ok(serde_json::to_vec(value).map_err(|e| e.to_string())?.len() as u64)
}

pub fn diagnostic(mut error: String) -> String {
    let mut end = error.len().min(2048);
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    error.truncate(end);
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_capacity_limits_reject_zero_excess_and_unknown_fields() {
        for value in [
            serde_json::json!({"max_pending":0}),
            serde_json::json!({"max_pending_per_reservation":33}),
            serde_json::json!({"max_calls_per_reservation":1}),
            serde_json::json!({"max_dispatch_workers":0}),
            serde_json::json!({"max_transition_workers":0}),
            serde_json::json!({"max_gateway_waiters":129}),
            serde_json::json!({"max_wait_seconds":0}),
            serde_json::json!({"max_response_bytes":4194305}),
            serde_json::json!({"retention_admission_bytes":33554432}),
            serde_json::json!({"terminal_evidence_bytes":1}),
        ] {
            assert!(
                serde_json::from_value::<Limits>(value)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        assert!(serde_json::from_value::<Limits>(serde_json::json!({"unbounded":true})).is_err());
        assert!(Limits::default().validate().is_ok());
    }
}
