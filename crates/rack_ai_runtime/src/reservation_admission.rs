use crate::{
    admission::{Admission, eligibility},
    config::Source,
    reservation::Reserve,
    service::Service,
    types::*,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub struct ReservationAdmission<'a> {
    pub service: &'a Service,
    pub source: &'a Source,
}
impl ReservationAdmission<'_> {
    pub fn reserve(&self, request: Reserve) -> Result<Value, String> {
        self.service.authority.update(|s| {
            if let Some(d) = s.data.demands.values().find(|d| {
                d.owner == self.source.source
                    && d.reserve_request
                        .as_ref()
                        .is_some_and(|r| r.acquisition_id == request.acquisition_id)
            }) {
                if d.reserve_request.as_ref() != Some(&request) {
                    return Err("identity_conflict".into());
                }
                return d
                    .reserve_result
                    .clone()
                    .ok_or("original_reservation_receipt_missing".into());
            }
            if let Some((archived_request, result)) =
                crate::history_archive::lookup_reservation_replay(
                    &self.service.config.authority_root,
                    &self.source.source,
                    &request.acquisition_id,
                )?
            {
                return if archived_request == request {
                    Ok(result)
                } else {
                    Err("identity_conflict".into())
                };
            }
            crate::history_archive::maintain(&self.service.config.authority_root, s, now())?;
            if !valid_id(&request.acquisition_id)
                || !valid_id(&request.work_id)
                || request.services.is_empty()
                || request.services.len() > self.service.config.profiles.len()
                || request.services.iter().collect::<BTreeSet<_>>().len() != request.services.len()
            {
                return Err("invalid_request".into());
            }
            let admission = Admission {
                service: self.service,
                source: self.source,
            };
            let mut demands = Vec::new();
            let mut physical = BTreeSet::new();
            for tag in &request.services {
                let profile = self
                    .service
                    .config
                    .profiles
                    .iter()
                    .find(|p| &p.tag == tag)
                    .ok_or("unknown_tag")?;
                for resource in &profile.resources {
                    if !physical.insert(resource.clone()) {
                        return Err("atomic_resource_overlap".into());
                    }
                }
                let acquisition_id = format!(
                    "reserve-{}",
                    digest(
                        format!("{}/{}/{tag}", self.source.source, request.acquisition_id)
                            .as_bytes()
                    )
                );
                demands.push(admission.prepare(
                    Acquire {
                        schema: VERSION.into(),
                        source_system: self.source.source.clone(),
                        work_id: request.work_id.clone(),
                        acquisition_id,
                        tag: tag.clone(),
                        priority: Some(request.priority),
                        capabilities: profile.capabilities.clone(),
                        context_tokens: profile.context_tokens,
                        ttl_seconds: request.ttl_seconds,
                        qualification: false,
                    },
                    s.data.demands.len() as u64 + demands.len() as u64,
                )?);
            }
            let root_id = demands.first().ok_or("invalid_request")?.id.clone();
            let services = demands
                .iter()
                .map(|d| (d.profile.tag.clone(), d.id.clone()))
                .collect::<BTreeMap<_, _>>();
            for demand in &mut demands {
                demand.reservation_id = Some(root_id.clone());
            }
            let cache_key = group_decision_key(&self.source.source, &request, &demands)?;
            let environment = group_environment(s)?;
            trim_group_decisions(s);
            let refusal = s
                .data
                .acquisition_decisions
                .get(&cache_key)
                .filter(|decision| {
                    decision.owner == self.source.source
                        && decision.environment == environment
                        && decision.expires_at > now()
                })
                .map(|decision| decision.reason.clone())
                .or_else(|| {
                    demands.iter().find_map(|d| {
                        eligibility(d).or_else(|| {
                            (crate::planner::Planner {
                                service: self.service,
                            })
                            .refusal(s, d)
                        })
                    })
                });
            if let Some(reason) = refusal {
                s.data.acquisition_decisions.insert(
                    cache_key,
                    AcquisitionDecision {
                        owner: self.source.source.clone(),
                        environment,
                        reason: reason.clone(),
                        retry_after: 2,
                        expires_at: now() + 2,
                    },
                );
                trim_group_decisions(s);
                return unavailable_group(&self.source.source, &request, demands, reason);
            }
            crate::planner::fence_group(s, &root_id, &mut demands)?;
            for demand in demands {
                s.data.demands.insert(demand.id.clone(), demand);
            }
            let root = s.data.demands.get_mut(&root_id).ok_or("not_found")?;
            root.services = services;
            root.reserve_request = Some(request);
            let result = crate::reservation_view::view(s, (&self.source.source, &root_id))?;
            s.data
                .demands
                .get_mut(&root_id)
                .ok_or("not_found")?
                .reserve_result = Some(result.clone());
            crate::capacity::retention(s, &self.service.config.limits)?;
            Ok(result)
        })
    }
}

/// Legacy single-member refresh support. New code must use an explicit new
/// acquisition after preemption; this helper never restores a preempted claim.
pub(crate) fn attempt(
    service: &Service,
    input: (&mut Document, &mut Demand),
) -> Result<(), String> {
    let (s, d) = input;
    if let Some(reason) =
        eligibility(d).or_else(|| (crate::planner::Planner { service }).refusal(s, d))
    {
        d.state = DemandState::Unavailable;
        d.reason = Some(reason);
        d.retry_after = Some(2);
    } else {
        crate::planner::fence(s, d)?;
    }
    Ok(())
}

fn group_decision_key(
    owner: &str,
    request: &Reserve,
    demands: &[Demand],
) -> Result<String, String> {
    let mut services = request.services.clone();
    services.sort();
    let material = serde_json::json!({
        "owner": owner, "services": services, "priority": request.priority,
        "ttl_seconds": request.ttl_seconds,
        "profiles": demands.iter().map(|d| &d.profile_hash).collect::<Vec<_>>(),
    });
    Ok(digest(
        &serde_json::to_vec(&material).map_err(|e| e.to_string())?,
    ))
}

fn group_environment(s: &Document) -> Result<String, String> {
    let claims = s
        .claims
        .iter()
        .map(|(resource, id)| {
            let demand = s.data.demands.get(id);
            serde_json::json!({"resource":resource,"owner":id,
            "state":demand.map(|d| d.state),"priority":demand.map(|d| d.priority),
            "generation":demand.map(|d| &d.generation)})
        })
        .collect::<Vec<_>>();
    Ok(digest(
        &serde_json::to_vec(&claims).map_err(|e| e.to_string())?,
    ))
}

fn trim_group_decisions(s: &mut Document) {
    let at = now();
    s.data
        .acquisition_decisions
        .retain(|_, decision| decision.expires_at > at);
    if s.data.acquisition_decisions.len() <= 128 {
        return;
    }
    let mut keys = s
        .data
        .acquisition_decisions
        .iter()
        .map(|(key, decision)| (decision.expires_at, key.clone()))
        .collect::<Vec<_>>();
    keys.sort();
    for (_, key) in keys
        .into_iter()
        .take(s.data.acquisition_decisions.len() - 128)
    {
        s.data.acquisition_decisions.remove(&key);
    }
}

fn unavailable_group(
    owner: &str,
    request: &Reserve,
    mut demands: Vec<Demand>,
    reason: String,
) -> Result<Value, String> {
    let root_seed = digest(format!("{owner}/{}", request.acquisition_id).as_bytes());
    let root = format!("unavailable-{}", &root_seed[..48]);
    let mut services = serde_json::Map::new();
    for demand in &mut demands {
        let seed =
            digest(format!("{owner}/{}/{}", request.acquisition_id, demand.profile.tag).as_bytes());
        demand.id = format!("unavailable-{}", &seed[..48]);
        demand.generation = seed.clone();
        demand.access_key = seed;
        demand.reservation_id = Some(root.clone());
        demand.state = DemandState::Unavailable;
        demand.reason = Some(reason.clone());
        demand.retry_after = Some(2);
        demand.deadline = now();
        let mut view = crate::api::public(demand.clone())?;
        view.as_object_mut()
            .ok_or("invalid_public_record")?
            .remove("gateway_path");
        services.insert(demand.profile.tag.clone(), view);
    }
    Ok(
        serde_json::json!({"id":root,"priority":request.priority,"state":"unavailable",
        "acquisition_id":request.acquisition_id,"requested_services":request.services,"services":services}),
    )
}
