use crate::{process, types::*};
use rack_ai_media::command::run;
use std::collections::BTreeSet;
#[path = "container_identity.rs"]
mod identity;

pub(super) fn resolve(d: &Demand) -> Result<Option<Process>, String> {
    let Some(observed) = observe(d)? else {
        require_saved_gone(d)?;
        return Ok(None);
    };
    if !observed.state.running {
        require_saved_gone(d)?;
        return Ok(None);
    }
    let mut p = process::capture(observed.state.pid, d.backend_activation())?;
    p.container = Some(observed.id);
    if let Some(saved) = &d.process
        && *saved != p
    {
        return Err("recovery_container_process_changed".into());
    }
    // Re-read the container after capturing the process to close replacement races.
    crate::container::verify(&p, &d.profile)?;
    Ok(Some(p))
}

pub(super) fn absent(d: &Demand) -> Result<(), String> {
    let Some(observed) = observe(d)? else {
        return require_saved_gone(d);
    };
    if observed.state.running {
        return Err("recovery_container_still_running".into());
    }
    require_saved_gone(d)?;
    // No force or volume deletion: Docker refuses removal if it became running.
    command(d, &["rm", &observed.id])?;
    if observe(d)?.is_some() {
        return Err("recovery_container_cleanup_unproven".into());
    }
    Ok(())
}

fn require_saved_gone(d: &Demand) -> Result<(), String> {
    if let Some(saved) = &d.process
        && !process::gone(saved)?
    {
        return Err("recovery_container_process_still_live".into());
    }
    Ok(())
}

fn command(d: &Demand, args: &[&str]) -> Result<String, String> {
    run(
        d.profile
            .executable
            .to_str()
            .ok_or("invalid_container_executable")?,
        args,
    )
}

fn observe(d: &Demand) -> Result<Option<identity::Container>, String> {
    let mut filters = vec![
        format!("name=^/rack-runtime-{}$", d.backend_activation()),
        format!("label=rack.activation={}", d.backend_activation()),
    ];
    if let Some(saved) = &d.process {
        let id = saved
            .container
            .as_deref()
            .ok_or("recovery_container_identity_missing")?;
        identity::valid_id(id)?;
        filters.push(format!("id={id}"));
    }
    let mut ids = BTreeSet::new();
    for filter in filters {
        let text = command(
            d,
            &[
                "ps",
                "-a",
                "--no-trunc",
                "--format",
                "{{.ID}}",
                "--filter",
                &filter,
            ],
        )?;
        for id in text.lines() {
            identity::valid_id(id)?;
            ids.insert(id.to_owned());
        }
    }
    if ids.len() > 1 {
        return Err("recovery_container_candidates_ambiguous".into());
    }
    let Some(id) = ids.into_iter().next() else {
        return Ok(None);
    };
    let text = command(d, &["inspect", &id])?;
    let mut values: Vec<identity::Container> =
        serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if values.len() != 1 {
        return Err("recovery_container_candidates_ambiguous".into());
    }
    let observed = values.pop().ok_or("recovery_container_missing")?;
    observed.verify(d, &id)?;
    Ok(Some(observed))
}
