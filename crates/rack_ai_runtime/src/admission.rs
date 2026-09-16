use crate::{config::Source, service::Service, types::*};
pub struct Admission<'a> {
    pub service: &'a Service,
    pub source: &'a Source,
}
impl Admission<'_> {
    pub fn acquire(&self, request: Acquire) -> Result<Demand, String> {
        let service = self.service;
        if request.source_system != self.source.source {
            return Err("source_spoofing".into());
        }
        service.authority.update(|s| self.acquire_in(s, request))
    }
    pub(crate) fn acquire_in(&self, s: &mut Document, request: Acquire) -> Result<Demand, String> {
        let service = self.service;
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
        let refusal = eligibility(&demand)
            .or_else(|| crate::planner::Planner { service }.refusal(s, &demand));
        if let Some(reason) = refusal {
            demand.state = DemandState::Denied;
            demand.reason = Some(reason);
        } else {
            crate::planner::fence(s, &mut demand)?;
        }
        s.data.demands.insert(demand.id.clone(), demand.clone());
        crate::capacity::retention(s, &service.config.limits)?;
        Ok(demand)
    }
    pub(crate) fn prepare(&self, request: Acquire, order: u64) -> Result<Demand, String> {
        let service = self.service;
        validate_request(&request, service.config.max_ttl_seconds)?;
        let priority = request.priority.unwrap_or(Priority::Low);
        if request.qualification && !self.source.qualification {
            return Err("qualification_mode_denied".into());
        }
        let profile = service
            .config
            .profiles
            .iter()
            .find(|p| p.tag == request.tag)
            .ok_or("unknown_tag")?
            .clone();
        let hash = digest(&serde_json::to_vec(&profile).map_err(|e| e.to_string())?);
        let demand = Demand {
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
        };
        Ok(demand)
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
        || r.capabilities
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != r.capabilities.len()
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
