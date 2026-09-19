use crate::{
    backend::BackendAccess,
    config::{Backend, Config, Driver, Profile},
    hosting::Hosting,
    service::{Service, owns},
    transition::current,
    types::*,
};

pub fn supported(profile: &Profile) -> bool {
    profile.backend == Backend::Vllm && profile.driver == Driver::Docker
}

pub fn backend_activation_for(s: &Document, d: &Demand) -> Result<String, String> {
    if !supported(&d.profile) {
        return Ok(d.generation.clone());
    }
    if let Some(record) = s
        .data
        .warm_residencies
        .values()
        .find(|record| compatible(record, d))
    {
        return Ok(record.id.clone());
    }
    identity()
}

pub fn retained_host_mib(s: &Document, d: &Demand) -> u64 {
    s.data
        .warm_residencies
        .values()
        .filter(|record| record.state == WarmResidencyState::Resident)
        .filter(|record| disjoint(record, d))
        .map(|record| record.profile.host_mib)
        .sum()
}

pub fn evict_incompatible(service: &Service, d: &Demand) -> Result<(), String> {
    let records = service.authority.update(|s| {
        let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
        if saved.generation != d.generation
            || saved.state != DemandState::Preparing
            || !owns(s, saved)
        {
            return Ok(Vec::new());
        }
        let ids = s
            .data
            .warm_residencies
            .iter()
            .filter(|(_, record)| record.state == WarmResidencyState::Resident)
            .filter(|(_, record)| overlaps(record, saved) && !compatible(record, saved))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &ids {
            let record = s
                .data
                .warm_residencies
                .get_mut(id)
                .ok_or("warm_residency_missing")?;
            record.state = WarmResidencyState::Evicting;
        }
        ids.iter()
            .map(|id| {
                s.data
                    .warm_residencies
                    .get(id)
                    .cloned()
                    .ok_or("warm_residency_missing".into())
            })
            .collect()
    })?;
    for record in records {
        evict_record(&service.config, &record)?;
        service.authority.update(|s| {
            remove_if_unchanged(s, &record);
            Ok(())
        })?;
    }
    Ok(())
}

pub fn matching_processes(s: &Document, d: &Demand) -> Vec<Process> {
    s.data
        .warm_residencies
        .values()
        .filter(|record| compatible(record, d))
        .map(|record| record.process.clone())
        .collect()
}

pub fn adopt_or_evict(service: &Service, d: &Demand) -> Result<bool, String> {
    let record = service.authority.read(|s| {
        let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
        if saved.generation != d.generation
            || saved.state != DemandState::Preparing
            || !owns(s, saved)
        {
            return Ok(None);
        }
        Ok(s.data
            .warm_residencies
            .get(saved.backend_activation())
            .filter(|record| record.state == WarmResidencyState::Resident)
            .cloned())
    })?;
    let Some(record) = record else {
        return Ok(false);
    };
    if verify_for_demand(&service.config, &record, d).is_ok() {
        return service.authority.update(|s| {
            let Some(live) = s.data.warm_residencies.get(&record.id).cloned() else {
                return Ok(false);
            };
            if live.state != WarmResidencyState::Resident
                || live.process != record.process
                || !compatible(&live, d)
            {
                return Ok(false);
            }
            let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
            if saved.generation != d.generation
                || saved.state != DemandState::Preparing
                || !owns(s, saved)
            {
                return Ok(false);
            }
            s.data.warm_residencies.remove(&record.id);
            let saved = current(s, d)?;
            saved.backend_activation = Some(record.id.clone());
            saved.process = Some(record.process.clone());
            saved.effect_started = true;
            Ok(true)
        });
    }
    mark_evicting(service, &record)?;
    evict_record(&service.config, &record)?;
    service.authority.update(|s| {
        remove_if_unchanged(s, &record);
        Ok(())
    })?;
    Ok(false)
}

pub fn cache_after_release(service: &Service, d: &Demand) -> Result<bool, String> {
    if !cache_candidate(d) {
        return Ok(false);
    }
    (BackendAccess {
        config: &service.config,
    })
    .ready(d)?;
    let process = d.process.clone().ok_or("warm_process_missing")?;
    let record = WarmResidency {
        id: process.activation.clone(),
        profile_hash: d.profile_hash.clone(),
        profile: d.profile.clone(),
        backend: d.profile.backend.clone(),
        driver: d.profile.driver.clone(),
        model: d.profile.model.clone(),
        resources: d.profile.resources.clone(),
        endpoint: d.profile.endpoint.clone(),
        process,
        cached_at: now(),
        last_ready_at: now(),
        state: WarmResidencyState::Resident,
    };
    service.authority.update(|s| {
        let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
        if saved.generation != d.generation
            || saved.process.as_ref() != Some(&record.process)
            || !cache_candidate(saved)
            || saved.backend_activation() != record.id
        {
            return Ok(false);
        }
        if s.data
            .warm_residencies
            .values()
            .any(|existing| existing.id != record.id && overlaps(existing, saved))
        {
            return Err("warm_residency_overlap".into());
        }
        s.data.warm_residencies.insert(record.id.clone(), record);
        let saved = current(s, d)?;
        saved.process = None;
        saved.effect_started = false;
        saved.released = true;
        saved.state = terminal_state(saved);
        s.claims.retain(|_, owner| owner != &d.id);
        Ok(true)
    })
}

pub fn reconcile_on_start(service: &Service) -> Result<(), String> {
    let records = service.authority.read(|s| {
        Ok(s.data
            .warm_residencies
            .values()
            .filter(|record| record.state == WarmResidencyState::Resident)
            .cloned()
            .collect::<Vec<_>>())
    })?;
    for record in records {
        if current_profile(&service.config, &record)
            .and_then(|profile| verify_for_profile(&service.config, &record, profile))
            .is_ok()
        {
            service.authority.update(|s| {
                if let Some(saved) = s.data.warm_residencies.get_mut(&record.id)
                    && saved.process == record.process
                    && saved.state == WarmResidencyState::Resident
                {
                    saved.last_ready_at = now();
                }
                Ok(())
            })?;
            continue;
        }
        mark_evicting(service, &record)?;
        evict_record(&service.config, &record)?;
        service.authority.update(|s| {
            remove_if_unchanged(s, &record);
            Ok(())
        })?;
    }
    Ok(())
}

fn cache_candidate(d: &Demand) -> bool {
    supported(&d.profile)
        && d.process.is_some()
        && d.effect_started
        && d.state == DemandState::Releasing
        && !matches!(
            d.reason.as_deref(),
            Some("cancelled" | "preempted_by_higher_priority")
        )
}

fn compatible(record: &WarmResidency, d: &Demand) -> bool {
    record.state == WarmResidencyState::Resident
        && record.profile_hash == d.profile_hash
        && record.process.activation == record.id
        && record.resources == d.profile.resources
}

fn overlaps(record: &WarmResidency, d: &Demand) -> bool {
    record
        .resources
        .iter()
        .any(|resource| d.profile.resources.contains(resource))
}

fn disjoint(record: &WarmResidency, d: &Demand) -> bool {
    !overlaps(record, d)
}

fn verify_for_demand(
    config: &Config,
    record: &WarmResidency,
    demand: &Demand,
) -> Result<(), String> {
    if !compatible(record, demand) || demand.backend_activation() != record.id {
        return Err("warm_residency_profile_mismatch".into());
    }
    let mut adopted = demand.clone();
    adopted.process = Some(record.process.clone());
    adopted.effect_started = true;
    adopted.backend_activation = Some(record.id.clone());
    (BackendAccess { config }).ready(&adopted)
}

fn verify_for_profile(
    _config: &Config,
    record: &WarmResidency,
    profile: &Profile,
) -> Result<(), String> {
    if !supported(profile) {
        return Err("warm_residency_profile_mismatch".into());
    }
    crate::process::endpoint_owned(&record.process, profile)
}

fn current_profile<'a>(config: &'a Config, record: &WarmResidency) -> Result<&'a Profile, String> {
    let profile = config
        .profiles
        .iter()
        .find(|profile| profile.tag == record.profile.tag)
        .ok_or("warm_residency_profile_missing")?;
    let hash = crate::types::digest(&serde_json::to_vec(profile).map_err(|e| e.to_string())?);
    if hash != record.profile_hash {
        return Err("warm_residency_profile_changed".into());
    }
    Ok(profile)
}

fn mark_evicting(service: &Service, record: &WarmResidency) -> Result<(), String> {
    service.authority.update(|s| {
        if let Some(saved) = s.data.warm_residencies.get_mut(&record.id)
            && saved.process == record.process
        {
            saved.state = WarmResidencyState::Evicting;
        }
        Ok(())
    })
}

fn evict_record(config: &Config, record: &WarmResidency) -> Result<(), String> {
    Hosting { config }.stop(&synthetic_demand(record))
}

fn remove_if_unchanged(s: &mut Document, record: &WarmResidency) {
    if s.data
        .warm_residencies
        .get(&record.id)
        .is_some_and(|saved| saved.process == record.process)
    {
        s.data.warm_residencies.remove(&record.id);
    }
}

fn terminal_state(d: &Demand) -> DemandState {
    match d.reason.as_deref() {
        Some("cancelled") => DemandState::Cancelled,
        Some("preempted_by_higher_priority") => DemandState::Preempted,
        Some(crate::idle::IDLE_TIMEOUT) => DemandState::Expired,
        _ if d.deadline <= now() => DemandState::Expired,
        _ => DemandState::Released,
    }
}

fn synthetic_demand(record: &WarmResidency) -> Demand {
    Demand {
        reservation_id: None,
        services: Default::default(),
        reserve_request: None,
        reserve_result: None,
        reservation_closed: None,
        recovery_reconciliation: None,
        recovery_error: None,
        id: format!("warm-{}", record.id),
        owner: "rack-ai".into(),
        request: Acquire {
            schema: VERSION.into(),
            source_system: "rack-ai".into(),
            work_id: "warm-residency".into(),
            acquisition_id: record.id.clone(),
            tag: record.profile.tag.clone(),
            priority: Some(Priority::Low),
            capabilities: record.profile.capabilities.clone(),
            context_tokens: record.profile.context_tokens,
            ttl_seconds: 1,
            qualification: true,
        },
        priority: Priority::Low,
        profile: record.profile.clone(),
        profile_hash: record.profile_hash.clone(),
        state: DemandState::Ready,
        reason: None,
        retry_after: None,
        preempted_by: None,
        ready_checked: true,
        accepted_calls: 0,
        generation: record.id.clone(),
        access_key: record.id.clone(),
        backend_activation: Some(record.id.clone()),
        created: record.cached_at,
        last_activity_at: Some(record.last_ready_at),
        order: 0,
        deadline: now() + 1,
        transition_deadline: now() + record.profile.stop_seconds,
        victims: vec![],
        process: Some(record.process.clone()),
        effect_started: true,
        preflight_done: true,
        released: false,
    }
}
