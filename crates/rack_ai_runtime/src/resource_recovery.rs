//! Operator-triggered retry of existing cleanup; never grants or steals a claim.
use crate::{
    config::Backend,
    control::{Action, Control, ControlContext, ReservationControl},
    service::Service,
    types::*,
};
use ResourceStatus::*;
pub use rack_ai_media::resource_command::{Report, Resource, ResourceStatus, SCHEMA};
pub fn run(service: &Service, apply: bool) -> Result<Report, String> {
    let mut resources = Vec::new();
    for id in service.config.devices.keys() {
        // Keep the established media-operation -> authority lock order.
        let operation = media_operation(service, id);
        if operation.is_err() {
            resources.push(entry(service, id, BlockedPhysicalVerification));
            continue;
        }
        let _operation = operation?;
        let action = |s: &mut Document| {
            if let Some(owner) = s.claims.get(id) {
                let Some(d) = s.data.demands.get(owner).cloned() else {
                    return Ok(BlockedUnprovenOwner);
                };
                if !retryable(&d) {
                    return Ok(if d.state == DemandState::RecoveryRequired {
                        UnresolvedWorkPreserved
                    } else {
                        InUsePreserved
                    });
                }
                if s.data.invocations.values().any(|i| {
                    i.request.reservation_id == d.id
                        && matches!(i.state, InvocationState::Queued | InvocationState::Running)
                }) {
                    return Ok(ActiveWorkPreserved);
                }
                if apply {
                    let closed = d.reservation_closed;
                    ReservationControl { service }.apply(
                        s,
                        ControlContext {
                            owner: &d.owner,
                            id: &d.id,
                            request: Control {
                                generation: d.generation.clone(),
                                action: Action::Release,
                            },
                        },
                    )?;
                    s.data
                        .demands
                        .get_mut(&d.id)
                        .ok_or("missing recovery member")?
                        .reservation_closed = closed;
                }
                return Ok(if apply {
                    CleanupRequested
                } else {
                    CleanupPending
                });
            }
            if let Some(warm) = s
                .data
                .warm_residencies
                .values_mut()
                .find(|w| w.resources.contains(id))
            {
                if warm.state == WarmResidencyState::Resident {
                    return Ok(WarmPreserved);
                }
                if apply {
                    warm.state = WarmResidencyState::Evicting;
                }
                return Ok(if apply {
                    CleanupRequested
                } else {
                    CleanupPending
                });
            }
            let profiles: Vec<_> = service
                .config
                .profiles
                .iter()
                .filter(|p| p.backend == Backend::Comfyui && p.resources.contains(id))
                .collect();
            if profiles.is_empty() {
                return Ok(Available);
            }
            // Multiple logical profiles may share one native receiver.
            let paths: std::collections::BTreeSet<_> = profiles
                .iter()
                .filter_map(|p| p.media_config.clone())
                .collect();
            if paths.len() != 1 {
                return Ok(BlockedMediaBinding);
            }
            let profile = profiles
                .iter()
                .find(|p| p.native_media())
                .unwrap_or(&profiles[0]);
            crate::resource_unit_recovery::reset(service, (profile, apply))
        };
        let status = if apply {
            service.authority.update(action)?
        } else {
            service.authority.read(|s| action(&mut s.clone()))?
        };
        resources.push(entry(service, id, status));
    }
    Ok(Report {
        schema: SCHEMA.into(),
        resources,
    })
}
fn retryable(d: &Demand) -> bool {
    d.released && d.state == DemandState::RecoveryRequired && d.recovery_reconciliation.is_none()
}
fn entry(service: &Service, id: &str, status: ResourceStatus) -> Resource {
    Resource {
        resource: id.into(),
        services: service
            .config
            .profiles
            .iter()
            .filter(|p| p.qualified && p.resources.iter().any(|r| r == id))
            .map(|p| p.tag.clone())
            .collect(),
        status,
    }
}
fn media_operation(service: &Service, id: &str) -> Result<Option<std::fs::File>, String> {
    let Some(profile) = service
        .config
        .profiles
        .iter()
        .find(|p| p.backend == Backend::Comfyui && p.resources.iter().any(|r| r == id))
    else {
        return Ok(None);
    };
    let config = rack_ai_media::config::Config::load(
        profile
            .media_config
            .clone()
            .ok_or("media configuration missing")?,
    )?;
    rack_ai_media::operation::lock(&config.state_root).map(Some)
}
