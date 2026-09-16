use crate::{
    admission::Admission, config::Source, reservation::Reserve, service::Service, types::*,
};
use serde_json::Value;
use std::collections::BTreeSet;
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
            for tag in &request.services {
                let p = self
                    .service
                    .config
                    .profiles
                    .iter()
                    .find(|p| &p.tag == tag)
                    .ok_or("unknown_tag")?;
                let acquisition_id = format!(
                    "reserve-{}",
                    digest(
                        format!("{}/{}/{tag}", self.source.source, request.acquisition_id)
                            .as_bytes()
                    )
                );
                if s.data.demands.values().any(|d| {
                    d.owner == self.source.source && d.request.acquisition_id == acquisition_id
                }) {
                    return Err("identity_conflict".into());
                }
                demands.push(admission.prepare(
                    Acquire {
                        schema: VERSION.into(),
                        source_system: self.source.source.clone(),
                        work_id: request.work_id.clone(),
                        acquisition_id,
                        tag: tag.clone(),
                        priority: Some(request.priority),
                        capabilities: p.capabilities.clone(),
                        context_tokens: p.context_tokens,
                        ttl_seconds: request.ttl_seconds,
                        qualification: false,
                    },
                    s.data.demands.len() as u64 + demands.len() as u64,
                )?);
            }
            let id = demands.first().ok_or("invalid_request")?.id.clone();
            let services = demands
                .iter()
                .map(|d| (d.profile.tag.clone(), d.id.clone()))
                .collect();
            for mut d in demands {
                d.reservation_id = Some(id.clone());
                attempt(self.service, (s, &mut d))?;
                s.data.demands.insert(d.id.clone(), d);
            }
            let root = s.data.demands.get_mut(&id).ok_or("not_found")?;
            root.services = services;
            root.reserve_request = Some(request);
            let result = crate::reservation_view::view(s, (&self.source.source, &id))?;
            s.data
                .demands
                .get_mut(&id)
                .ok_or("not_found")?
                .reserve_result = Some(result.clone());
            crate::capacity::retention(s, &self.service.config.limits)?;
            Ok(result)
        })
    }
}
pub(crate) fn attempt(
    service: &Service,
    input: (&mut Document, &mut Demand),
) -> Result<(), String> {
    let (s, d) = input;
    let refusal = crate::admission::eligibility(d)
        .or_else(|| (crate::planner::Planner { service }).refusal(s, d));
    if let Some(reason) = refusal {
        d.state = DemandState::Denied;
        d.reason = Some(reason);
    } else {
        crate::planner::fence(s, d)?;
        d.reason = None;
    }
    Ok(())
}
