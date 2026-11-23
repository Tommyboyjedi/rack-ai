//! Reconcile an invocation whose dispatch worker no longer holds its record lock.
use crate::{
    config::Backend,
    service::{Service, active, owns},
    types::*,
};

pub(crate) const MISSING_RESULT: &str = "invocation_outcome_unknown_backend_idle";
const UNPROVEN_DRAIN: &str = "invocation_worker_gone_backend_drain_unproven";

pub(crate) fn reconcile(service: &Service) -> Result<(), String> {
    reconcile_with(service, backend_idle)
}

pub(crate) fn backend_idle(
    config: &crate::config::Config,
    demand: &Demand,
) -> Result<bool, String> {
    match demand.profile.backend {
        Backend::Strata => crate::strata_ready::idle(config, demand),
        Backend::Ninfer => (crate::backend::BackendAccess { config }).ninfer_idle(demand),
        _ => Ok(false),
    }
}

pub(crate) fn reconcile_with(
    service: &Service,
    probe_idle: impl Fn(&crate::config::Config, &Demand) -> Result<bool, String>,
) -> Result<(), String> {
    let candidates = service.authority.read(|state| {
        Ok(state
            .data
            .invocations
            .values()
            .filter_map(|invocation| {
                let demand = state.data.demands.get(&invocation.request.reservation_id)?;
                eligible(state, invocation, demand).then(|| (invocation.clone(), demand.clone()))
            })
            .collect::<Vec<_>>())
    })?;
    for (invocation, demand) in candidates {
        let Some(_worker) = crate::workers::record_lock(service, &invocation.id)? else {
            continue;
        };
        let idle = probe_idle(&service.config, &demand).unwrap_or(false);
        let deadline_elapsed = invocation
            .execution_deadline
            .is_some_and(|end| end <= now());
        if !idle && !deadline_elapsed {
            continue;
        }
        let changed = service
            .authority
            .update(|state| Ok(finish(state, (&invocation, &demand), idle)))?;
        if changed {
            service.request_history_maintenance();
        }
    }
    Ok(())
}

fn eligible(state: &Document, invocation: &Invocation, demand: &Demand) -> bool {
    invocation.state == InvocationState::Running
        && !crate::work_payload::is_workspace(invocation)
        && invocation.activation.as_ref() == Some(&demand.generation)
        && matches!(demand.profile.backend, Backend::Strata | Backend::Ninfer)
        && demand.state == DemandState::Ready
        && active(demand)
        && owns(state, demand)
}

fn finish(state: &mut Document, expected: (&Invocation, &Demand), idle: bool) -> bool {
    let (invocation, demand) = expected;
    let Some(saved) = state.data.invocations.get(&invocation.id) else {
        return false;
    };
    let Some(current) = state.data.demands.get(&demand.id) else {
        return false;
    };
    if !eligible(state, saved, current)
        || saved.activation != invocation.activation
        || saved.started != invocation.started
        || current.generation != demand.generation
        || current.profile_hash != demand.profile_hash
        || current.process != demand.process
    {
        return false;
    }
    let saved = state.data.invocations.get_mut(&invocation.id).unwrap();
    // Failed denotes a terminal request with the backend proven idle. Its
    // diagnostic preserves the unknown result without quarantining this service.
    saved.state = if idle {
        InvocationState::Failed
    } else {
        InvocationState::Uncertain
    };
    saved.error = Some(if idle { MISSING_RESULT } else { UNPROVEN_DRAIN }.into());
    saved.completed = Some(now());
    true
}
