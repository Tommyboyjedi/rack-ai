//! Cross-process permits are taken before spawning; held work creates no workers or lock files.
use crate::{
    service::{Service, active, inflight, owns},
    types::*,
};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
pub fn eligible(s: &Document, i: &Invocation) -> bool {
    (!crate::work_payload::is_workspace(i)
        || !s.data.invocations.values().any(|other| {
            other.request.reservation_id == i.request.reservation_id
                && crate::work_payload::is_workspace(other)
                && matches!(
                    other.state,
                    InvocationState::Started | InvocationState::Uncertain
                )
        }))
        && i.state == InvocationState::Accepted
        && i.waiting_deadline > now()
        && crate::workspace_scope::permits(s, &i.request)
        && s.data
            .demands
            .get(&i.request.reservation_id)
            .is_some_and(|d| {
                d.state == DemandState::Ready
                    && active(d)
                    && owns(s, d)
                    && crate::reservation::ready(s, d)
                    && !inflight(s, &d.id)
            })
}
pub fn permit(service: &Service, pool: Pool) -> Result<Option<File>, String> {
    let (kind, count) = match pool {
        Pool::Dispatch => ("dispatch", service.config.limits.max_dispatch_workers),
        Pool::Transition => ("transition", service.config.limits.max_transition_workers),
    };
    let root = service.config.authority_root.join("worker-slots");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for index in 0..count {
        if let Some(file) = lock(&root.join(format!("{kind}-{index}")))? {
            return Ok(Some(file));
        }
    }
    Ok(None)
}
pub fn lock(path: &Path) -> Result<Option<File>, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn record_lock(service: &Service, id: &str) -> Result<Option<File>, String> {
    let root = service.config.authority_root.join("workers");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    lock(&root.join(id))
}

pub enum Pool {
    Dispatch,
    Transition,
}
