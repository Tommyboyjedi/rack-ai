use crate::{
    config::Backend,
    protocol::Protocol,
    service::{Service, owned},
    types::Demand,
};
use subtle::ConstantTimeEq;

pub(super) struct Access {
    pub owner: String,
    pub reservation: String,
    pub capability: String,
}

pub(super) fn read(service: &Service, access: &Access) -> Result<Demand, String> {
    service.authority.read(|state| {
        let demand = owned(state, &access.owner, &access.reservation)?;
        if !bool::from(
            demand
                .access_key
                .as_bytes()
                .ct_eq(access.capability.as_bytes()),
        ) {
            return Err("invalid_scoped_capability".into());
        }
        eligible(state, demand)?;
        Ok(demand.clone())
    })
}

pub(super) fn verify(service: &Service, expected: &Demand) -> Result<(), String> {
    service.authority.read(|state| {
        let current = owned(state, &expected.owner, &expected.id)?;
        eligible(state, current)?;
        if current.generation != expected.generation
            || current.profile_hash != expected.profile_hash
            || current.process != expected.process
            || current.backend_activation() != expected.backend_activation()
        {
            return Err("stale_generation_or_profile".into());
        }
        Ok(())
    })
}

fn eligible(state: &crate::types::Document, demand: &Demand) -> Result<(), String> {
    if !crate::reservation::ready(state, demand) {
        return Err("reservation_not_dispatchable".into());
    }
    if (!demand.profile.qualified && !demand.request.qualification)
        || !demand.profile.input_token_preflight
        || !matches!(demand.profile.backend, Backend::LlamaCpp | Backend::Strata)
        || !demand
            .profile
            .protocols
            .contains(&Protocol::ChatCompletions)
    {
        return Err("input_preflight_unqualified".into());
    }
    let encoded = serde_json::to_vec(&demand.profile).map_err(|_| "input_preflight_unqualified")?;
    if crate::types::digest(&encoded) != demand.profile_hash {
        return Err("stale_generation_or_profile".into());
    }
    Ok(())
}
