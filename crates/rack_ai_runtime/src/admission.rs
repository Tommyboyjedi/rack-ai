use crate::{config::Source, service::Service, types::*};
use std::collections::BTreeSet;

const DECISION_CACHE_SECONDS: u64 = 2;
const MAX_DECISION_CACHE_ENTRIES: usize = 128;

pub struct Admission<'a> {
    pub service: &'a Service,
    pub source: &'a Source,
}
impl Admission<'_> {
    pub fn acquire(&self, request: Acquire) -> Result<Demand, String> {
        if request.source_system != self.source.source {
            return Err("source_spoofing".into());
        }
        self.service
            .authority
            .update(|s| self.acquire_in(s, request))
    }

    pub(crate) fn acquire_in(&self, s: &mut Document, request: Acquire) -> Result<Demand, String> {
        if let Some(d) = s.data.demands.values().find(|d| {
            d.owner == self.source.source && d.request.acquisition_id == request.acquisition_id
        }) {
            return if d.request == request {
                Ok(d.clone())
            } else {
                Err("identity_conflict".into())
            };
        }
        let mut demand = self.prepare(request, s.data.demands.len() as u64)?;
        if let Some(reason) = eligibility(&demand) {
            return Ok(self.unavailable(demand, reason, None));
        }
        let key = decision_key(&self.source.source, &demand)?;
        let environment = decision_environment(s, &demand)?;
        purge_decisions(s);
        if let Some(decision) = s.data.acquisition_decisions.get(&key)
            && decision.owner == self.source.source
            && decision.environment == environment
            && decision.expires_at > now()
        {
            return Ok(self.unavailable(
                demand,
                decision.reason.clone(),
                Some(decision.retry_after),
            ));
        }
        if let Some(reason) = (crate::planner::Planner {
            service: self.service,
        })
        .refusal(s, &demand)
        {
            let retry_after = DECISION_CACHE_SECONDS;
            s.data.acquisition_decisions.insert(
                key,
                AcquisitionDecision {
                    owner: self.source.source.clone(),
                    environment,
                    reason: reason.clone(),
                    retry_after,
                    expires_at: now() + retry_after,
                },
            );
            purge_decisions(s);
            return Ok(self.unavailable(demand, reason, Some(retry_after)));
        }
        crate::planner::fence(s, &mut demand)?;
        s.data.demands.insert(demand.id.clone(), demand.clone());
        crate::capacity::retention(s, &self.service.config.limits)?;
        Ok(demand)
    }

    fn unavailable(&self, mut demand: Demand, reason: String, retry_after: Option<u64>) -> Demand {
        let identity_seed = format!(
            "{}/{}/{}",
            self.source.source, demand.request.acquisition_id, demand.request.tag
        );
        let digest = digest(identity_seed.as_bytes());
        demand.id = format!("unavailable-{}", &digest[..48]);
        demand.generation = digest.clone();
        demand.access_key = digest;
        demand.state = DemandState::Unavailable;
        demand.reason = Some(reason);
        demand.retry_after = retry_after;
        demand.deadline = now();
        demand.transition_deadline = 0;
        demand
    }

    pub(crate) fn prepare(&self, request: Acquire, order: u64) -> Result<Demand, String> {
        validate_request(&request, self.service.config.max_ttl_seconds)?;
        let priority = request.priority.unwrap_or(Priority::Low);
        if request.qualification && !self.source.qualification {
            return Err("qualification_mode_denied".into());
        }
        let profile = self
            .service
            .config
            .profiles
            .iter()
            .find(|p| p.tag == request.tag)
            .ok_or("unknown_tag")?
            .clone();
        let hash = digest(&serde_json::to_vec(&profile).map_err(|e| e.to_string())?);
        Ok(Demand {
            reservation_id: None,
            services: Default::default(),
            reserve_request: None,
            reserve_result: None,
            reservation_closed: None,
            id: identity()?,
            owner: self.source.source.clone(),
            request: request.clone(),
            priority,
            profile,
            profile_hash: hash,
            state: DemandState::Preparing,
            reason: None,
            retry_after: None,
            preempted_by: None,
            ready_checked: false,
            accepted_calls: 0,
            generation: identity()?,
            access_key: identity()?,
            created: now(),
            last_activity_at: None,
            order,
            deadline: now() + request.ttl_seconds,
            transition_deadline: 0,
            victims: vec![],
            process: None,
            effect_started: false,
            preflight_done: false,
            released: false,
        })
    }
}

fn validate_request(r: &Acquire, max_ttl: u64) -> Result<(), String> {
    if r.schema != VERSION
        || !valid_id(&r.acquisition_id)
        || !valid_id(&r.work_id)
        || r.ttl_seconds == 0
        || r.ttl_seconds > max_ttl
        || r.context_tokens == 0
        || r.capabilities.is_empty()
        || r.capabilities.iter().collect::<BTreeSet<_>>().len() != r.capabilities.len()
    {
        return Err("invalid_request".into());
    }
    Ok(())
}

pub(crate) fn eligibility(d: &Demand) -> Option<String> {
    if !d.profile.qualified && !d.request.qualification {
        return Some("unqualified_profile".into());
    }
    if d.request.context_tokens > d.profile.context_tokens
        || d.request
            .capabilities
            .iter()
            .any(|c| !d.profile.capabilities.contains(c))
    {
        return Some("capability_or_context_unqualified".into());
    }
    None
}

fn decision_key(owner: &str, demand: &Demand) -> Result<String, String> {
    // acquisition_id is deliberately absent: callers commonly rotate it while
    // polling a denied-now request. It must not manufacture retained demands.
    let material = serde_json::json!({
        "owner": owner, "tag": demand.request.tag, "priority": demand.priority,
        "capabilities": demand.request.capabilities, "context_tokens": demand.request.context_tokens,
        "ttl_seconds": demand.request.ttl_seconds, "qualification": demand.request.qualification,
        "profile_hash": demand.profile_hash,
    });
    Ok(digest(
        &serde_json::to_vec(&material).map_err(|e| e.to_string())?,
    ))
}

fn decision_environment(s: &Document, demand: &Demand) -> Result<String, String> {
    let owners = demand
        .profile
        .resources
        .iter()
        .map(|resource| {
            let incumbent = s.claims.get(resource).and_then(|id| s.data.demands.get(id));
            serde_json::json!({
                "resource": resource,
                "claim": s.claims.get(resource),
                "state": incumbent.map(|d| d.state),
                "priority": incumbent.map(|d| d.priority),
                "generation": incumbent.map(|d| &d.generation),
            })
        })
        .collect::<Vec<_>>();
    Ok(digest(
        &serde_json::to_vec(&owners).map_err(|e| e.to_string())?,
    ))
}

fn purge_decisions(s: &mut Document) {
    let at = now();
    s.data
        .acquisition_decisions
        .retain(|_, decision| decision.expires_at > at);
    if s.data.acquisition_decisions.len() <= MAX_DECISION_CACHE_ENTRIES {
        return;
    }
    let mut oldest = s
        .data
        .acquisition_decisions
        .iter()
        .map(|(key, decision)| (decision.expires_at, key.clone()))
        .collect::<Vec<_>>();
    oldest.sort();
    for (_, key) in oldest
        .into_iter()
        .take(s.data.acquisition_decisions.len() - MAX_DECISION_CACHE_ENTRIES)
    {
        s.data.acquisition_decisions.remove(&key);
    }
}
