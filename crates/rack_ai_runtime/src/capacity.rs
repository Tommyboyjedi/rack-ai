//! Admission reserves future encoded output and cleanup space; retained evidence is never pruned.
use crate::types::*;
use serde::Deserialize;
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_pending: usize,
    pub max_pending_per_reservation: usize,
    pub max_dispatch_workers: usize,
    pub max_transition_workers: usize,
    pub max_gateway_waiters: usize,
    pub max_wait_seconds: u64,
    pub max_response_bytes: u64,
    pub retention_admission_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pending: 32,
            max_pending_per_reservation: 16,
            max_dispatch_workers: 4,
            max_transition_workers: 8,
            max_gateway_waiters: 32,
            max_wait_seconds: 300,
            max_response_bytes: 256 * 1024,
            retention_admission_bytes: 30 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_pending == 0
            || self.max_pending > 1024
            || self.max_pending_per_reservation == 0
            || self.max_pending_per_reservation > self.max_pending
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
        {
            return Err("invalid_runtime_capacity_limits".into());
        }
        Ok(())
    }
}
impl Limits {
    pub fn pending(&self, s: &Document, reservation: &str) -> Result<(), String> {
        let pending: Vec<_> = s
            .data
            .invocations
            .values()
            .filter(|i| {
                matches!(
                    i.state,
                    InvocationState::Accepted | InvocationState::Started
                )
            })
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
        Ok(())
    }
}

pub fn retention(s: &Document, limits: &Limits) -> Result<(), String> {
    let mut reserved = serde_json::to_vec(s).map_err(|e| e.to_string())?.len() as u64;
    // JSON escaping can expand a raw protocol response by six; envelope/diagnostics are bounded.
    for i in s.data.invocations.values() {
        if matches!(
            i.state,
            InvocationState::Accepted | InvocationState::Started
        ) {
            reserved = reserved.saturating_add(i.response_bytes.saturating_mul(6) + 16384);
        }
    }
    // Each demand retains headroom for claims, process identity, victims and terminal diagnostics.
    // Reserve for terminal demands too: repeated controls must remain writable without deletion.
    for d in s.data.demands.values() {
        let profile = serde_json::to_vec(&d.profile)
            .map_err(|e| e.to_string())?
            .len() as u64;
        reserved = reserved.saturating_add(65536 + profile * 4 + s.data.demands.len() as u64 * 64);
    }
    // Every retained scope reserves a bounded close timestamp, even without a submission.
    reserved = reserved.saturating_add(s.data.workspace_scopes.len() as u64 * 128);
    if reserved > limits.retention_admission_bytes {
        return Err("capacity_retained_evidence".into());
    }
    Ok(())
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
            serde_json::json!({"max_dispatch_workers":0}),
            serde_json::json!({"max_dispatch_workers":65}),
            serde_json::json!({"max_transition_workers":0}),
            serde_json::json!({"max_gateway_waiters":129}),
            serde_json::json!({"max_wait_seconds":0}),
            serde_json::json!({"max_wait_seconds":86401}),
            serde_json::json!({"max_response_bytes":4194305}),
            serde_json::json!({"retention_admission_bytes":33554432}),
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
