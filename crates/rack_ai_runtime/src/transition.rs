use crate::{
    backend::BackendAccess,
    hosting::Hosting,
    preflight::Preflight,
    service::{Service, active, inflight, owns},
    types::*,
};
pub struct Transition<'a> {
    pub service: &'a Service,
}
impl Transition<'_> {
    pub fn advance(&self, d: &Demand) -> Result<(), String> {
        let r = self.service;
        if d.state == DemandState::RecoveryRequired {
            return (crate::recovery::Recovery { service: r }).advance(d);
        }
        if !active(d) || d.state == DemandState::Releasing {
            return crate::retirement::Retirement { service: r }.run(d);
        }
        if d.state == DemandState::Ready {
            return ReadyMonitor { service: r }.advance(d).or_else(|error| {
                if crate::media_evidence::supported(d) {
                    (crate::media_recovery::MediaRecovery { service: r }).advance(d, Some(&error))
                } else {
                    Err(error)
                }
            });
        }
        if d.state != DemandState::Preparing {
            return Ok(());
        }
        self.boundary(d)?;
        if !d.preflight_done {
            crate::residency::evict_incompatible(r, d)?;
            let (victims, residents) = r.authority.read(|s| {
                Ok((
                    d.victims
                        .iter()
                        .filter_map(|v| s.data.demands.get(v))
                        .cloned()
                        .collect::<Vec<_>>(),
                    crate::residency::matching_processes(s, d),
                ))
            })?;
            if let Err(e) = (Preflight { config: &r.config })
                .check_with_resident_processes(d, &victims, &residents)
            {
                if e == "legacy_dispatch_still_draining" && now() < d.transition_deadline {
                    return Ok(());
                }
                return Err(e);
            }
            return r.authority.update(|s| {
                let saved = current(s, d)?;
                if !active(saved) {
                    return Ok(());
                }
                saved.preflight_done = true;
                saved.transition_deadline = now()
                    + d.profile.startup_seconds
                    + d.profile.drain_seconds
                    + d.profile.stop_seconds;
                Ok(())
            });
        }
        if !(VictimDrain { service: r }).advance(d)? {
            return Ok(());
        }
        if d.process.is_none() {
            if crate::residency::adopt_or_evict(r, d)? {
                return Ok(());
            }
            Preflight { config: &r.config }.empty(&d.profile)?;
            let allowed = r.authority.update(|s| {
                let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
                if !active(saved) || saved.state != DemandState::Preparing || !owns(s, saved) {
                    return Ok(false);
                }
                current(s, d)?.effect_started = true;
                Ok(true)
            })?;
            if !allowed {
                return Ok(());
            }
            let process = Hosting { config: &r.config }.start(d)?;
            return r.authority.update(|s| {
                let saved = current(s, d)?;
                saved.process = Some(process);
                Ok(())
            });
        }
        ReadinessCommit { service: r }.advance(d)
    }
    fn boundary(&self, d: &Demand) -> Result<(), String> {
        self.service.authority.read(|s| {
            let saved = s.data.demands.get(&d.id).ok_or("missing_transition")?;
            if saved.generation != d.generation
                || saved.state != DemandState::Preparing
                || !active(saved)
            {
                return Err("transition_fence_changed".into());
            }
            Ok(())
        })
    }
}
pub fn current<'a>(s: &'a mut Document, d: &Demand) -> Result<&'a mut Demand, String> {
    let saved = s.data.demands.get_mut(&d.id).ok_or("missing_transition")?;
    if saved.generation != d.generation {
        return Err("stale_transition_callback".into());
    }
    Ok(saved)
}

struct VictimDrain<'a> {
    service: &'a Service,
}
impl VictimDrain<'_> {
    fn advance(&self, d: &Demand) -> Result<bool, String> {
        let r = self.service;
        // A multi-service reservation shares one incumbent set. Only the root
        // member performs stop-and-mark-preempted; its peers wait for that
        // durable fence before claiming their independent resources.
        if d.reservation_id.as_deref().is_some_and(|root| root != d.id) {
            let drained = r.authority.read(|s| {
                let root = s
                    .data
                    .demands
                    .get(
                        d.reservation_id
                            .as_deref()
                            .ok_or("missing_reservation_root")?,
                    )
                    .ok_or("missing_reservation_root")?;
                Ok(root.victims.iter().all(|id| {
                    s.data
                        .demands
                        .get(id)
                        .is_some_and(|victim| victim.state == DemandState::Preempted)
                }))
            })?;
            if !drained {
                return Ok(false);
            }
        }
        for id in &d.victims {
            let victim = r.authority.read(|s| {
                let victim = s.data.demands.get(id).ok_or("missing_victim")?.clone();
                if victim.state != DemandState::Preempted && inflight(s, id) {
                    if now() >= victim.transition_deadline {
                        return Err("drain_deadline_invocation_uncertain".into());
                    }
                    return Ok(None);
                }
                Ok(Some(victim))
            })?;
            let Some(victim) = victim else {
                return Ok(false);
            };
            if victim.state == DemandState::Preempted {
                continue;
            }
            if victim.state != DemandState::Preempting {
                return Err("victim_transition_changed".into());
            }
            Transition { service: r }.boundary(d)?;
            Hosting { config: &r.config }.stop(&victim)?;
            return r
                .authority
                .update(|s| {
                    let v = s.data.demands.get_mut(id).ok_or("missing_victim")?;
                    if v.generation != victim.generation || v.state != DemandState::Preempting {
                        return Err("stale_victim_callback".into());
                    }
                    v.process = None;
                    v.effect_started = false;
                    v.state = DemandState::Preempted;
                    v.reason = Some("preempted_by_higher_priority".into());
                    // The old owner is terminal. Remove every physical claim it held;
                    // the incoming candidate claims only its own required resources.
                    s.claims.retain(|_, owner| owner != id);
                    Ok(())
                })
                .map(|_| false);
        }
        r.authority.update(|s| {
            let candidate = s
                .data
                .demands
                .get(&d.id)
                .ok_or("missing_transition")?
                .clone();
            if candidate.generation != d.generation || candidate.state != DemandState::Preparing {
                return Err("transition_fence_changed".into());
            }
            crate::planner::transfer(s, &candidate)
        })?;
        Ok(true)
    }
}

struct ReadinessCommit<'a> {
    service: &'a Service,
}
impl ReadinessCommit<'_> {
    fn advance(&self, d: &Demand) -> Result<(), String> {
        let r = self.service;
        let media = d.profile.backend == crate::config::Backend::Comfyui
            && d.profile.driver != crate::config::Driver::Fixture;
        match (BackendAccess {
            config: &self.service.config,
        })
        .ready(d)
        {
            Ok(()) => {
                let media_process = if media {
                    Some(
                        (crate::media::MediaAdapter { config: &r.config })
                            .observe(d)?
                            .1,
                    )
                } else {
                    None
                };
                r.authority.update(|s| {
                    if !active(current(s, d)?) {
                        return Ok(());
                    }
                    current(s, d)?.ready_checked = true;
                    if let Some(p) = media_process {
                        current(s, d)?.process = Some(p);
                    }
                    crate::reservation::commit_ready(s, d)
                })
            }
            Err(_)
                if media
                    && (crate::media::MediaAdapter { config: &r.config }).restarting(d)?
                    && now() < d.transition_deadline =>
            {
                Ok(())
            }
            Err(_)
                if media
                    && (crate::media_recovery::MediaRecovery { service: r })
                        .retire_stopped(d)? =>
            {
                Ok(())
            }
            Err(_)
                if media
                    && now() < d.transition_deadline
                    && (crate::media::MediaAdapter { config: &r.config })
                        .awaiting_identity(d)? =>
            {
                Ok(())
            }
            Err(error) if media => {
                (crate::media_recovery::MediaRecovery { service: r }).advance(d, Some(&error))
            }
            Err(_) if now() < d.transition_deadline => Ok(()),
            Err(error) => {
                r.authority.update(|s| {
                    let saved = current(s, d)?;
                    saved.released = true;
                    saved.reason = Some(error);
                    Ok(())
                })?;
                crate::retirement::Retirement { service: r }.run(d)
            }
        }
    }
}

struct ReadyMonitor<'a> {
    service: &'a Service,
}
impl ReadyMonitor<'_> {
    fn advance(&self, d: &Demand) -> Result<(), String> {
        let r = self.service;
        let media = d.profile.backend == crate::config::Backend::Comfyui
            && d.profile.driver != crate::config::Driver::Fixture;
        let adapter = crate::media::MediaAdapter { config: &r.config };
        let ready = BackendAccess { config: &r.config }.ready(d);
        if media && adapter.restarting(d)? {
            return r.authority.update(|s| {
                let saved = current(s, d)?;
                if saved.state == DemandState::Ready && active(saved) {
                    saved.state = DemandState::Preparing;
                    saved.transition_deadline = now() + d.profile.startup_seconds;
                }
                Ok(())
            });
        }
        ready?;
        if media {
            crate::media_idle::MediaIdle { service: r }.observe(d)?;
            // Idle admission may just have closed and started normal teardown.
            if !r.authority.read(|s| {
                Ok(crate::reservation::ready(
                    s,
                    crate::service::owned(s, &d.owner, &d.id)?,
                ))
            })? {
                return Ok(());
            }
            let process = adapter.observe(d)?.1;
            r.authority.update(|s| {
                let saved = current(s, d)?;
                if saved.state == DemandState::Ready {
                    saved.process = Some(process);
                }
                Ok(())
            })?;
        }
        Ok(())
    }
}
