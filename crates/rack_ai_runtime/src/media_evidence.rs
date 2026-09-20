//! Reconcile media only from canonical ownership and OS evidence, never HTTP health.
use crate::{
    media::MediaAdapter,
    service::{Service, active, owns},
    types::*,
};
use rack_ai_media::{
    runtime::Runtime,
    types::{Mode, Service as MediaService, ServiceState},
};

pub struct MediaEvidence<'a> {
    pub service: &'a Service,
    pub runtime: &'a Runtime,
}
impl MediaEvidence<'_> {
    pub fn bound(&self, d: &Demand) -> Result<MediaService, String> {
        self.service.authority.read(|s| {
            let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
            if saved.generation != d.generation || !owns(s, saved) {
                return Err("media_recovery_claim_changed".into());
            }
            Ok(())
        })?;
        let p = d.process.as_ref().ok_or("media_recovery_process_unknown")?;
        if p.activation != d.generation || p.unit.as_ref() != Some(&self.runtime.config.unit) {
            return Err("media_recovery_process_binding_changed".into());
        }
        let state = self.runtime.store.read()?;
        let media = state.service;
        if media.state == ServiceState::Stopped
            && media.lease.is_none()
            && media.activation.is_empty()
        {
            if state
                .jobs
                .iter()
                .any(|j| !j.state.terminal() || j.cleanup_pending)
                || state
                    .sessions
                    .iter()
                    .any(|s| !s.stopped && !s.release_requested)
            {
                return Err("media_recovery_pending_work".into());
            }
            return Ok(media);
        }
        let handle = media.lease.as_ref().ok_or("media_recovery_lease_missing")?;
        if media.activation != d.generation
            || handle.owner != d.id
            || handle.generation != d.generation
        {
            return Err("media_recovery_activation_changed".into());
        }
        self.runtime.reservations.verify(handle)?;
        Ok(media)
    }
    pub fn stopped(&self, d: &Demand) -> Result<bool, String> {
        let media = self.bound(d)?;
        if media
            .restart
            .as_ref()
            .is_some_and(|r| r.phase != rack_ai_media::restart_state::RestartPhase::Completed)
        {
            return Ok(false);
        }
        if !self.runtime.systemd.gone()? {
            return Ok(false);
        }
        let mut process = d.process.clone().ok_or("media_recovery_process_unknown")?;
        if !crate::process::gone(&process)? {
            return Err("media_recovery_recorded_process_alive".into());
        }
        if let Some(generation) = media.generation {
            process.pid = generation.pid;
            process.start = generation.start_ticks.to_string();
            if !crate::process::gone(&process)? {
                return Err("media_recovery_generation_alive".into());
            }
        }
        if !(rack_ai_media::gpu::GpuProbe {
            config: &self.runtime.config,
        })
        .pids()?
        .is_empty()
        {
            return Err("media_recovery_gpu_not_clear".into());
        }
        Ok(true)
    }
    /// Prove that the previously attempted media effect is absent now. Every
    /// probe is authoritative for this profile, and any unreadable or changed
    /// evidence is an error rather than a reason to release the claim.
    pub fn effect_absent(&self, d: &Demand) -> Result<RecoveryAbsenceChecks, String> {
        absence_authority(self.service, d)?;
        if d.process.is_some() {
            return Err("media_recovery_recorded_process_present".into());
        }
        let state = self.runtime.store.read()?;
        let media = state.service;
        if media.state != ServiceState::Stopped
            || media.mode != Mode::Closed
            || !media.activation.is_empty()
            || media.invocation.is_some()
            || media.generation.is_some()
            || media.restart.is_some()
            || media.lease.is_some()
        {
            return Err("media_recovery_lifecycle_still_active".into());
        }
        if state
            .jobs
            .iter()
            .any(|j| !j.state.terminal() || j.cleanup_pending)
        {
            return Err("media_recovery_pending_work".into());
        }
        if state.sessions.iter().any(|session| !session.stopped) {
            return Err("media_recovery_session_still_active".into());
        }
        if !self.runtime.systemd.gone()? {
            return Err("media_recovery_systemd_activation_present".into());
        }
        if !(rack_ai_media::gpu::GpuProbe {
            config: &self.runtime.config,
        })
        .pids()?
        .is_empty()
        {
            return Err("media_recovery_gpu_not_clear".into());
        }
        Ok(RecoveryAbsenceChecks {
            reservation_inactive: true,
            active_invocations_absent: true,
            recorded_process_absent: true,
            systemd_activation_absent: true,
            gpu_allocation_absent: true,
            media_session_absent: true,
            lifecycle_transition_absent: true,
            ownership_fence_intact: true,
        })
    }
    pub fn owned(&self, d: &Demand) -> Result<MediaService, String> {
        let media = self.bound(d)?;
        let observed = self.runtime.systemd.observe()?;
        if observed.pending_job || media.invocation.as_ref() != Some(&observed.invocation) {
            return Err("media_recovery_invocation_changed".into());
        }
        media
            .generation
            .as_ref()
            .ok_or("media_recovery_generation_unknown")?
            .verify(&observed)?;
        (rack_ai_media::gpu::GpuProbe {
            config: &self.runtime.config,
        })
        .owned(&observed)?;
        Ok(media)
    }
}
/// The authority half of effect-absence reconciliation. It checks only durable
/// ownership/lifecycle facts, while effect_absent checks the independently
/// authoritative media, systemd, and GPU observations.
pub fn absence_authority(service: &Service, d: &Demand) -> Result<(), String> {
    service.authority.read(|s| absence_authority_document(s, d))
}

pub fn absence_authority_document(s: &Document, d: &Demand) -> Result<(), String> {
    let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
    if saved.generation != d.generation || !owns(s, saved) {
        return Err("media_recovery_claim_changed".into());
    }
    if saved.state != DemandState::RecoveryRequired
        || saved.reason.as_deref() != Some("start_outcome_unknown")
    {
        return Err("media_recovery_historical_outcome_not_supported".into());
    }
    if saved.process.is_some() || !saved.effect_started {
        return Err("media_recovery_claim_changed".into());
    }
    if !saved.released || active(saved) {
        return Err("media_recovery_reservation_still_active".into());
    }
    let root = crate::reservation::root(s, saved)?;
    if root.id != saved.id
        && !(root.released
            && !active(root)
            && matches!(
                root.state,
                DemandState::Released | DemandState::Cancelled | DemandState::Expired
            ))
    {
        return Err("media_recovery_reservation_still_active".into());
    }
    let members = crate::reservation::members(s, saved)?;
    if s.data.invocations.values().any(|invocation| {
        members
            .iter()
            .any(|id| id == &invocation.request.reservation_id)
            && matches!(
                invocation.state,
                InvocationState::Queued | InvocationState::Running
            )
    }) {
        return Err("media_recovery_active_invocation_present".into());
    }
    if s.data.demands.values().any(|other| {
        if other.id == saved.id {
            return false;
        }
        let overlaps = other
            .profile
            .resources
            .iter()
            .any(|resource| saved.profile.resources.contains(resource));
        matches!(
            other.state,
            DemandState::Preparing
                | DemandState::Ready
                | DemandState::Preempting
                | DemandState::Releasing
        ) && (other.victims.iter().any(|victim| victim == &saved.id) || (active(other) && overlaps))
    }) {
        return Err("media_recovery_lifecycle_transition_in_progress".into());
    }
    Ok(())
}
pub fn supported(d: &Demand) -> bool {
    d.profile.backend == crate::config::Backend::Comfyui
        && d.profile.driver != crate::config::Driver::Fixture
}
pub fn runtime(service: &Service, d: &Demand) -> Result<Runtime, String> {
    (MediaAdapter {
        config: &service.config,
    })
    .runtime(d)
}
pub fn transport(error: &str) -> bool {
    matches!(
        error,
        "backend transport uncertain" | "backend_transport_uncertain" | "backend read uncertain"
    ) || error.starts_with("backend HTTP 5")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn demand() -> Demand {
        serde_json::from_value(json!({
            "id": "historical-demand",
            "owner": "cb",
            "request": {
                "schema": VERSION,
                "source_system": "cb",
                "work_id": "historical-work",
                "acquisition_id": "historical-acquisition",
                "tag": "comfyui",
                "priority": null,
                "capabilities": [],
                "context_tokens": 1,
                "ttl_seconds": 60,
                "qualification": false
            },
            "priority": "paramount",
            "profile": {
                "tag": "comfyui",
                "version": "fixture",
                "model": "",
                "backend": "comfyui",
                "driver": "systemd",
                "qualified": true,
                "evidence": [],
                "capabilities": [],
                "context_tokens": 1,
                "max_input_tokens": 1,
                "max_output_tokens": 1,
                "resources": ["gpu-4080-super"],
                "device_mib": {},
                "host_mib": 1,
                "cpu_percent": 1,
                "endpoint": "http://127.0.0.1:1",
                "executable": "/bin/true",
                "executable_sha256": "",
                "args": [],
                "artifact": null,
                "artifact_sha256": null,
                "startup_seconds": 1,
                "drain_seconds": 1,
                "stop_seconds": 1,
                "inference_seconds": 1
            },
            "profile_hash": "fixture",
            "state": "recovery_required",
            "reason": "start_outcome_unknown",
            "generation": "historical-generation",
            "access_key": "synthetic",
            "created": 1,
            "order": 1,
            "deadline": now() + 60,
            "transition_deadline": 1,
            "victims": [],
            "process": null,
            "effect_started": true,
            "preflight_done": true,
            "released": true
        }))
        .unwrap()
    }

    fn document(demand: Demand) -> Document {
        let mut claims = BTreeMap::new();
        claims.insert("gpu-4080-super".into(), demand.id.clone());
        let mut data = State::default();
        data.demands.insert(demand.id.clone(), demand);
        Document { claims, data }
    }

    fn invocation(demand: &Demand, state: InvocationState) -> Invocation {
        let state = match state {
            InvocationState::Queued => "queued",
            InvocationState::Running => "running",
            InvocationState::Uncertain => "uncertain",
            _ => unreachable!(),
        };
        serde_json::from_value(json!({
            "id": format!("{}-{}", state, demand.id),
            "owner": "cb",
            "request": {
                "schema": VERSION,
                "submission_id": state,
                "reservation_id": demand.id,
                "generation": demand.generation,
                "profile_hash": demand.profile_hash,
                "prompt": "historical fixture",
                "max_tokens": 1,
                "timeout_seconds": 1
            },
            "state": state,
            "waiting_deadline": now() + 60
        }))
        .unwrap()
    }

    #[test]
    fn effect_absence_keeps_an_active_reservation_fenced() {
        let mut demand = demand();
        demand.released = false;
        let error = absence_authority_document(&document(demand.clone()), &demand).unwrap_err();
        assert_eq!(error, "media_recovery_reservation_still_active");
    }

    #[test]
    fn effect_absence_keeps_active_work_fenced() {
        let demand = demand();
        let mut document = document(demand.clone());
        let active = invocation(&demand, InvocationState::Running);
        document.data.invocations.insert(active.id.clone(), active);
        let error = absence_authority_document(&document, &demand).unwrap_err();
        assert_eq!(error, "media_recovery_active_invocation_present");
    }

    #[test]
    fn effect_absence_preserves_historical_uncertain_work() {
        let demand = demand();
        let mut document = document(demand.clone());
        let historical = invocation(&demand, InvocationState::Uncertain);
        document
            .data
            .invocations
            .insert(historical.id.clone(), historical);
        absence_authority_document(&document, &demand).unwrap();
        assert_eq!(
            document.data.invocations["uncertain-historical-demand"].state,
            InvocationState::Uncertain
        );
    }

    #[test]
    fn effect_absence_keeps_an_overlapping_transition_fenced() {
        let demand = demand();
        let mut document = document(demand.clone());
        let mut candidate = demand.clone();
        candidate.id = "candidate".into();
        candidate.state = DemandState::Preparing;
        candidate.released = false;
        candidate.reason = None;
        document
            .data
            .demands
            .insert(candidate.id.clone(), candidate);
        let error = absence_authority_document(&document, &demand).unwrap_err();
        assert_eq!(error, "media_recovery_lifecycle_transition_in_progress");
    }

    #[test]
    fn effect_absence_accepts_a_terminal_legacy_reservation_root() {
        let mut child = demand();
        child.reservation_id = Some("legacy-root".into());
        let mut document = document(child.clone());
        let mut root = child.clone();
        root.id = "legacy-root".into();
        root.reservation_id = Some(root.id.clone());
        root.state = DemandState::Expired;
        root.reason = Some("idle_timeout".into());
        root.effect_started = false;
        root.services = BTreeMap::from([("comfyui".into(), child.id.clone())]);
        document.data.demands.insert(root.id.clone(), root);
        absence_authority_document(&document, &child).unwrap();
    }
}
