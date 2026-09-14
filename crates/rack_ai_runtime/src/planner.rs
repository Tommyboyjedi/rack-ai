use crate::{service::Service, types::*};
use std::collections::BTreeSet;
pub struct Planner<'a> {
    pub service: &'a Service,
}
impl Planner<'_> {
    pub fn refusal(&self, s: &Document, d: &Demand) -> Option<String> {
        match self
            .service
            .authority
            .resources
            .legacy_blocked(&d.profile.resources)
        {
            Ok(ids) if ids.is_empty() => (),
            Ok(ids) => return Some(format!("legacy_ownership_requires_migration:{ids:?}")),
            Err(e) => return Some(format!("ownership_uncertain:{e}")),
        }
        for id in conflicts(s, d) {
            let Some(incumbent) = s.data.demands.get(&id) else {
                return Some("ownership_uncertain".into());
            };
            if incumbent.priority >= d.priority {
                return Some(format!("incumbent_priority:{id}"));
            }
            if incumbent.state != DemandState::Ready {
                return Some(format!("transition_or_recovery_blocked:{id}"));
            }
        }
        let occupied_host: u64 = s
            .data
            .demands
            .values()
            .filter(|x| {
                matches!(
                    x.state,
                    DemandState::Ready
                        | DemandState::Releasing
                        | DemandState::Preparing
                        | DemandState::Draining
                        | DemandState::RecoveryRequired
                ) && !conflicts(s, d).contains(&x.id)
            })
            .map(|x| x.profile.host_mib)
            .sum();
        if occupied_host.saturating_add(d.profile.host_mib) > self.service.config.host_capacity_mib
        {
            return Some("insufficient_host_memory".into());
        }
        None
    }
}
fn conflicts(s: &Document, d: &Demand) -> BTreeSet<String> {
    d.profile
        .resources
        .iter()
        .filter_map(|r| s.claims.get(r))
        .filter(|id| **id != d.id)
        .cloned()
        .collect()
}
pub fn fence(s: &mut Document, d: &mut Demand) -> Result<(), String> {
    let victims = conflicts(s, d);
    let mut resources = d.profile.resources.clone();
    for id in &victims {
        let victim = s.data.demands.get_mut(id).ok_or("ownership_uncertain")?;
        victim.state = DemandState::Draining;
        victim.reason = Some(format!("preempted_by:{}", d.id));
        victim.transition_deadline = now() + victim.profile.drain_seconds;
        resources.extend(victim.profile.resources.clone());
    }
    for resource in resources {
        s.claims.insert(resource, d.id.clone());
    }
    d.victims = victims.into_iter().collect();
    d.generation = identity()?;
    d.access_key = identity()?;
    d.transition_deadline = now()
        + d.profile.startup_seconds
        + d.victims
            .iter()
            .filter_map(|v| s.data.demands.get(v))
            .map(|v| v.profile.drain_seconds + v.profile.stop_seconds)
            .sum::<u64>();
    d.state = DemandState::Preparing;
    d.effect_started = false;
    d.preflight_done = false;
    Ok(())
}
