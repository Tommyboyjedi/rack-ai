//! Rediscover only generation-owned effects; never start or replay work.
use crate::{config::Driver, types::*};
mod container;
mod fixture;
mod systemd;
#[cfg(test)]
mod tests;

pub fn resolve(d: &Demand) -> Result<Option<Process>, String> {
    validate(d)?;
    match d.profile.driver {
        Driver::Docker => container::resolve(d),
        Driver::Systemd => systemd::resolve(d),
        Driver::Fixture => fixture::resolve(d),
    }
}

/// Prove effect removal after stop; remove only a stopped, exactly owned container.
pub fn absent(d: &Demand) -> Result<(), String> {
    validate(d)?;
    if d.profile.driver == Driver::Docker {
        return container::absent(d);
    }
    if resolve(d)?.is_some() {
        return Err("recovery_owned_process_still_live".into());
    }
    Ok(())
}

fn validate(d: &Demand) -> Result<(), String> {
    let activation = d.backend_activation();
    if activation.is_empty()
        || activation.len() > 128
        || !activation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("recovery_invalid_generation".into());
    }
    if d.process
        .as_ref()
        .is_some_and(|p| p.activation != activation)
    {
        return Err("recovery_activation_changed".into());
    }
    Ok(())
}
