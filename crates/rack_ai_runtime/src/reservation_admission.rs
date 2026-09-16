use crate::{
    admission::Admission, config::Source, reservation::Reserve, service::Service, types::*,
};
use std::collections::{BTreeMap, BTreeSet};
pub struct ReservationAdmission<'a> {
    pub service: &'a Service,
    pub source: &'a Source,
}
impl ReservationAdmission<'_> {
    pub fn reserve(&self, request: Reserve) -> Result<String, String> {
        self.service.authority.update(|s| {
            if let Some(d) = s.data.demands.values().find(|d| {
                d.owner == self.source.source
                    && d.reserve_request
                        .as_ref()
                        .is_some_and(|r| r.acquisition_id == request.acquisition_id)
            }) {
                return if d.reserve_request.as_ref() == Some(&request) {
                    Ok(d.id.clone())
                } else {
                    Err("identity_conflict".into())
                };
            }
            if !valid_id(&request.acquisition_id)
                || !valid_id(&request.work_id)
                || request.services.is_empty()
                || request.services.len() > self.service.config.profiles.len()
                || request.services.iter().collect::<BTreeSet<_>>().len() != request.services.len()
            {
                return Err("invalid_request".into());
            }
            let mut demands = Vec::new();
            let mut resources = BTreeSet::new();
            for tag in &request.services {
                let p = self
                    .service
                    .config
                    .profiles
                    .iter()
                    .find(|p| &p.tag == tag)
                    .ok_or("unknown_tag")?;
                if p.resources.iter().any(|r| !resources.insert(r.clone())) {
                    return Err("services_require_conflicting_resources".into());
                }
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
                demands.push(
                    (Admission {
                        service: self.service,
                        source: self.source,
                    })
                    .prepare(
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
                    )?,
                );
            }
            let id = demands.first().ok_or("invalid_request")?.id.clone();
            let services: BTreeMap<_, _> = demands
                .iter()
                .map(|d| (d.profile.tag.clone(), d.id.clone()))
                .collect();
            crate::reservation::admit(s, (self.service, &mut demands))?;
            for mut d in demands {
                d.reservation_id = Some(id.clone());
                if d.id == id {
                    d.services = services.clone();
                    d.reserve_request = Some(request.clone());
                }
                s.data.demands.insert(d.id.clone(), d);
            }
            crate::capacity::retention(s, &self.service.config.limits)?;
            Ok(id)
        })
    }
}
