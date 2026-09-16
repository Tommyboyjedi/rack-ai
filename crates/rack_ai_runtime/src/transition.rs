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
        if d.state == DemandState::RecoveryRequired && crate::media_evidence::supported(d) {
            return (crate::media_recovery::MediaRecovery { service: r }).advance(d, None);
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
            let victims = r.authority.read(|s| {
                Ok(d.victims
                    .iter()
                    .filter_map(|v| s.data.demands.get(v))
                    .cloned()
                    .collect::<Vec<_>>())
            })?;
            if let Err(e) = (Preflight { config: &r.config }).check(d, &victims) {
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
                || !owns(s, saved)
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
        for id in &d.victims {
            let victim = r.authority.read(|s| {
                let victim = s.data.demands.get(id).ok_or("missing_victim")?.clone();
                if inflight(s, id) {
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
            if victim.state == DemandState::Held {
                continue;
            }
            Transition { service: r }.boundary(d)?;
            Hosting { config: &r.config }.stop(&victim)?;
            return r
                .authority
                .update(|s| {
                    let v = s.data.demands.get_mut(id).ok_or("missing_victim")?;
                    if v.generation != victim.generation {
                        return Err("stale_victim_callback".into());
                    }
                    v.process = None;
                    v.effect_started = false;
                    v.state = if active(v) {
                        DemandState::Held
                    } else {
                        DemandState::Releasing
                    };
                    Ok(())
                })
                .map(|_| false);
        }
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
                    current(s, d)?.state = DemandState::Ready;
                    if let Some(p) = media_process {
                        current(s, d)?.process = Some(p);
                    }
                    s.claims.retain(|resource, owner| {
                        owner != &d.id || d.profile.resources.contains(resource)
                    });
                    Ok(())
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
