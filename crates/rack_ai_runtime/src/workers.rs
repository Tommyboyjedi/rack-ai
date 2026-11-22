//! Cross-process permits are taken before spawning; held work creates no workers or lock files.
use crate::{
    service::{Service, active, inflight, owns},
    types::*,
};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
// An old background request joins the interactive lane before the usual
// five-minute queue deadline. Within each lane, the older admission wins.
const BACKGROUND_AGING_SECONDS: u64 = 60;

fn order(i: &Invocation, at: u64) -> (bool, u64, &str) {
    let fresh_background = i.priority == InvocationPriority::Background
        && (i.created == 0 || at.saturating_sub(i.created) < BACKGROUND_AGING_SECONDS);
    (fresh_background, i.queue_order, &i.id)
}

pub fn eligible(s: &Document, i: &Invocation) -> bool {
    let at = now();
    (!crate::work_payload::is_workspace(i)
        || !s.data.invocations.values().any(|other| {
            other.request.reservation_id == i.request.reservation_id
                && crate::work_payload::is_workspace(other)
                && matches!(
                    other.state,
                    InvocationState::Running | InvocationState::Uncertain
                )
        }))
        && i.state == InvocationState::Queued
        // Request ordering is local to a reservation and never changes ownership.
        && !s.data.invocations.values().any(|other| {
            other.request.reservation_id == i.request.reservation_id
                && other.state == InvocationState::Queued
                && other.waiting_deadline > at
                && order(other, at) < order(i, at)
        })
        && i.waiting_deadline > at
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
