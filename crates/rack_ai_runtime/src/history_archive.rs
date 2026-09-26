use crate::{reservation::Reserve, types::*};
use rack_ai_application::durable_file::atomic_write;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const ARCHIVE_SCHEMA: &str = "rack-ai/history-archive/v1";
const RETENTION_SECONDS: u64 = 14 * 24 * 60 * 60;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub archived_invocations: usize,
    pub archived_reservations: usize,
    pub expired_files: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct InvocationArchive {
    schema: String,
    owner: String,
    invocation_id: String,
    reservation_id: String,
    submission_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    work_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closed_at_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
    #[serde(default)]
    preserved: bool,
    record: Invocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ReservationArchive {
    schema: String,
    owner: String,
    reservation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acquisition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request: Option<Reserve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    closed_at_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
    #[serde(default)]
    preserved: bool,
    #[serde(default)]
    demands: Vec<Demand>,
    #[serde(default)]
    invocation_ids: Vec<String>,
    #[serde(default)]
    workspace_scopes: BTreeMap<String, crate::workspace_scope::WorkspaceScope>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct InvocationPointer {
    schema: String,
    owner: String,
    invocation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ReservationPointer {
    schema: String,
    owner: String,
    reservation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
}

#[allow(dead_code)]
pub fn maintain(root: &Path, s: &mut Document, at: u64) -> Result<MaintenanceReport, String> {
    let mut report = MaintenanceReport::default();
    for root_id in closed_roots(s) {
        if closed_group_eligible(s, &root_id, at) {
            retire_closed_group(root, s, &root_id, at, &mut report)?;
        }
    }
    for id in active_terminal_invocations(s, at) {
        let Some(invocation) = s.data.invocations.get(&id).cloned() else {
            continue;
        };
        archive_invocation(root, s, &invocation, None)?;
        s.data.invocations.remove(&id);
        report.archived_invocations += 1;
    }
    report.expired_files += expire(root, at)?;
    Ok(report)
}

#[allow(dead_code)]
pub fn pending(root: &Path, s: &Document, at: u64) -> bool {
    !closed_roots(s).is_empty()
        || !active_terminal_invocations(s, at).is_empty()
        || expired_archive_exists(root, at).unwrap_or(false)
}

#[derive(Clone)]
pub struct MaintenancePlan {
    at: u64,
    kind: MaintenancePlanKind,
}

#[derive(Clone)]
enum MaintenancePlanKind {
    ActiveInvocation {
        invocation: Invocation,
        reservation: Option<Demand>,
    },
    ClosedGroup {
        root: Demand,
        member_ids: Vec<String>,
        members: Vec<Demand>,
        invocations: Vec<Invocation>,
        workspace_scopes: BTreeMap<String, crate::workspace_scope::WorkspaceScope>,
    },
    Expire,
}

pub struct PreparedMaintenance {
    plan: MaintenancePlan,
    staged: Vec<StagedFile>,
    expired_files: usize,
}

#[derive(Clone)]
struct StagedFile {
    final_path: PathBuf,
    stage_path: PathBuf,
    base_contents: Option<String>,
}

pub fn plan_maintenance(
    root: &Path,
    s: &Document,
    at: u64,
) -> Result<Option<MaintenancePlan>, String> {
    for root_id in closed_roots(s) {
        if !closed_group_eligible(s, &root_id, at) {
            continue;
        }
        let root = s
            .data
            .demands
            .get(&root_id)
            .cloned()
            .ok_or("missing_retirement_root")?;
        let member_ids = member_ids(s, &root).ok_or("missing_retirement_member")?;
        let member_set = member_ids.iter().cloned().collect::<BTreeSet<_>>();
        let members = member_ids
            .iter()
            .filter_map(|id| s.data.demands.get(id).cloned())
            .collect::<Vec<_>>();
        let invocations = s
            .data
            .invocations
            .values()
            .filter(|i| member_set.contains(&i.request.reservation_id))
            .cloned()
            .collect::<Vec<_>>();
        let workspace_scopes = s
            .data
            .workspace_scopes
            .iter()
            .filter(|(_, scope)| member_set.contains(&scope.reservation_id))
            .map(|(id, scope)| (id.clone(), scope.clone()))
            .collect();
        return Ok(Some(MaintenancePlan {
            at,
            kind: MaintenancePlanKind::ClosedGroup {
                root,
                member_ids,
                members,
                invocations,
                workspace_scopes,
            },
        }));
    }
    for id in active_terminal_invocations(s, at) {
        let Some(invocation) = s.data.invocations.get(&id).cloned() else {
            continue;
        };
        return Ok(Some(MaintenancePlan {
            at,
            kind: MaintenancePlanKind::ActiveInvocation {
                reservation: s
                    .data
                    .demands
                    .get(&invocation.request.reservation_id)
                    .cloned(),
                invocation,
            },
        }));
    }
    if expired_archive_exists(root, at)? {
        return Ok(Some(MaintenancePlan {
            at,
            kind: MaintenancePlanKind::Expire,
        }));
    }
    Ok(None)
}

pub fn prepare_maintenance(
    root: &Path,
    plan: MaintenancePlan,
) -> Result<PreparedMaintenance, String> {
    let stage_id = identity()?;
    let mut staged = Vec::new();
    let mut expired_files = 0;
    match &plan.kind {
        MaintenancePlanKind::ActiveInvocation {
            invocation,
            reservation,
        } => {
            pause_maintenance_io_if_configured();
            stage_invocation_archive(
                root,
                &stage_id,
                invocation,
                reservation.as_ref(),
                None,
                true,
                &mut staged,
            )?;
        }
        MaintenancePlanKind::ClosedGroup {
            root: demand,
            member_ids,
            members,
            invocations,
            workspace_scopes,
        } => {
            pause_maintenance_io_if_configured();
            let member_set = member_ids.iter().cloned().collect::<BTreeSet<_>>();
            let mut staged_invocations = BTreeMap::new();
            for invocation in invocations {
                let archive = stage_invocation_archive(
                    root,
                    &stage_id,
                    invocation,
                    members
                        .iter()
                        .find(|member| member.id == invocation.request.reservation_id),
                    Some(plan.at),
                    false,
                    &mut staged,
                )?;
                staged_invocations.insert(archive.invocation_id.clone(), archive);
            }
            let reservation = closed_reservation_archive_from_snapshot(
                root,
                demand,
                member_ids,
                members,
                invocations,
                workspace_scopes,
                &member_set,
                plan.at,
            )?;
            stage_reservation_archive(root, &stage_id, &reservation, &mut staged)?;
            for id in &reservation.invocation_ids {
                let mut archive = if let Some(archive) = staged_invocations.get(id).cloned() {
                    archive
                } else if let Some(archive) = read_invocation(root, &reservation.owner, id)? {
                    archive
                } else {
                    continue;
                };
                apply_closure(
                    &mut archive.closed_at,
                    &mut archive.closed_at_source,
                    &mut archive.expires_at,
                    archive.preserved,
                    reservation.closed_at,
                );
                stage_invocation_archive_value(root, &stage_id, &archive, &mut staged)?;
                stage_invocation_indexes(root, &stage_id, &archive, &mut staged)?;
            }
        }
        MaintenancePlanKind::Expire => {
            pause_maintenance_io_if_configured();
            expired_files = expire(root, plan.at)?;
        }
    }
    Ok(PreparedMaintenance {
        plan,
        staged,
        expired_files,
    })
}

pub fn prepared_maintenance_eligible(
    s: &Document,
    prepared: &PreparedMaintenance,
) -> Result<bool, String> {
    Ok(match &prepared.plan.kind {
        MaintenancePlanKind::ActiveInvocation { invocation, .. } => {
            active_invocation_prepared_eligible(s, invocation, prepared.plan.at)
        }
        MaintenancePlanKind::ClosedGroup {
            root: demand,
            member_ids,
            members,
            invocations,
            ..
        } => closed_group_prepared_eligible(
            s,
            demand,
            member_ids,
            members,
            invocations,
            prepared.plan.at,
        ),
        MaintenancePlanKind::Expire => true,
    })
}

fn active_invocation_prepared_eligible(s: &Document, invocation: &Invocation, at: u64) -> bool {
    s.data.invocations.get(&invocation.id).is_some_and(|live| {
        serialized_equal(live, invocation) && active_invocation_dependencies_clear(s, live, at)
    })
}

fn closed_group_prepared_eligible(
    s: &Document,
    demand: &Demand,
    member_ids: &[String],
    members: &[Demand],
    invocations: &[Invocation],
    at: u64,
) -> bool {
    let members_match = member_ids.iter().all(|id| {
        let Some(live) = s.data.demands.get(id) else {
            return false;
        };
        members
            .iter()
            .find(|member| member.id == *id)
            .is_some_and(|snapshot| serialized_equal(live, snapshot))
    });
    let invocations_match = invocations.iter().all(|snapshot| {
        s.data
            .invocations
            .get(&snapshot.id)
            .is_some_and(|live| serialized_equal(live, snapshot))
    });
    s.data
        .demands
        .get(&demand.id)
        .is_some_and(|live| serialized_equal(live, demand))
        && members_match
        && invocations_match
        && closed_group_eligible(s, &demand.id, at)
}

pub fn publish_prepared_maintenance(prepared: &PreparedMaintenance) -> Result<(), String> {
    publish_staged(&prepared.staged)
}

pub fn discard_prepared_maintenance(prepared: &PreparedMaintenance) {
    discard_staged(&prepared.staged);
}

pub fn commit_published_maintenance(
    s: &mut Document,
    prepared: PreparedMaintenance,
) -> Result<MaintenanceReport, String> {
    let mut report = MaintenanceReport::default();
    if !prepared_maintenance_eligible(s, &prepared)? {
        return Ok(report);
    }
    match prepared.plan.kind {
        MaintenancePlanKind::ActiveInvocation { invocation, .. } => {
            s.data.invocations.remove(&invocation.id);
            report.archived_invocations = 1;
        }
        MaintenancePlanKind::ClosedGroup {
            member_ids,
            invocations,
            ..
        } => {
            for invocation in invocations {
                s.data.invocations.remove(&invocation.id);
                report.archived_invocations += 1;
            }
            let member_set = member_ids.iter().cloned().collect::<BTreeSet<_>>();
            s.data
                .workspace_scopes
                .retain(|_, scope| !member_set.contains(&scope.reservation_id));
            for id in &member_ids {
                s.data.demands.remove(id);
            }
            report.archived_reservations = 1;
        }
        MaintenancePlanKind::Expire => {
            report.expired_files = prepared.expired_files;
        }
    }
    Ok(report)
}

pub fn lookup_invocation(root: &Path, owner: &str, id: &str) -> Result<Option<Invocation>, String> {
    let Some(archive) = read_invocation(root, owner, id)? else {
        return Ok(None);
    };
    if expired(archive.expires_at, now()) {
        return Ok(None);
    }
    crate::payload_store::hydrate_invocation(root, archive.record).map(Some)
}

pub fn lookup_work(root: &Path, owner: &str, work_id: &str) -> Result<Option<Invocation>, String> {
    let path = work_index_path(root, owner, work_id);
    let Some(pointer): Option<InvocationPointer> = read_json(&path)? else {
        return Ok(None);
    };
    if pointer.owner != owner || expired(pointer.expires_at, now()) {
        return Ok(None);
    }
    lookup_invocation(root, owner, &pointer.invocation_id)
}

pub fn lookup_submission(
    root: &Path,
    owner: &str,
    reservation_id: &str,
    submission_id: &str,
) -> Result<Option<Invocation>, String> {
    let path = submission_index_path(root, owner, reservation_id, submission_id);
    let Some(pointer): Option<InvocationPointer> = read_json(&path)? else {
        return Ok(None);
    };
    if pointer.owner != owner || expired(pointer.expires_at, now()) {
        return Ok(None);
    }
    lookup_invocation(root, owner, &pointer.invocation_id)
}

pub fn lookup_reservation_replay(
    root: &Path,
    owner: &str,
    acquisition_id: &str,
) -> Result<Option<(Reserve, Value)>, String> {
    let path = acquisition_index_path(root, owner, acquisition_id);
    let Some(pointer): Option<ReservationPointer> = read_json(&path)? else {
        return Ok(None);
    };
    if pointer.owner != owner || expired(pointer.expires_at, now()) {
        return Ok(None);
    }
    let Some(archive) = read_reservation(root, owner, &pointer.reservation_id)? else {
        return Ok(None);
    };
    if expired(archive.expires_at, now()) {
        return Ok(None);
    }
    Ok(archive.request.zip(archive.result))
}

pub fn lookup_reservation_view(
    root: &Path,
    owner: &str,
    reservation_id: &str,
) -> Result<Option<Value>, String> {
    let Some(archive) = read_reservation(root, owner, reservation_id)? else {
        return Ok(None);
    };
    if archive.owner != owner || expired(archive.expires_at, now()) {
        return Ok(None);
    }
    Ok(archive.result)
}

pub fn lookup_demand(root: &Path, owner: &str, id: &str) -> Result<Option<Demand>, String> {
    for archive in candidate_reservation_archives(root, owner)? {
        if archive.owner != owner || expired(archive.expires_at, now()) {
            continue;
        }
        if let Some(demand) = archive.demands.into_iter().find(|d| d.id == id) {
            return Ok(Some(demand));
        }
    }
    Ok(None)
}

fn closed_roots(s: &Document) -> Vec<String> {
    s.data
        .demands
        .values()
        .filter(|d| rootish(d) && d.reservation_closed.is_some())
        .map(|d| d.id.clone())
        .collect()
}

fn rootish(d: &Demand) -> bool {
    d.reservation_id.as_deref().is_none_or(|root| root == d.id)
}

fn closed_group_eligible(s: &Document, root_id: &str, at: u64) -> bool {
    let Some(root) = s.data.demands.get(root_id) else {
        return false;
    };
    if root.reservation_closed.is_none() {
        return false;
    }
    let Some(member_ids) = member_ids(s, root) else {
        return false;
    };
    let members = member_ids
        .iter()
        .filter_map(|id| s.data.demands.get(id))
        .collect::<Vec<_>>();
    if members.len() != member_ids.len() || !members.iter().all(|d| demand_retirable(s, d)) {
        return false;
    }
    let member_set = member_ids.iter().cloned().collect::<BTreeSet<_>>();
    if !scopes_closed_or_expired(s, &member_set, at) {
        return false;
    }
    s.data
        .invocations
        .values()
        .filter(|i| member_set.contains(&i.request.reservation_id))
        .all(|i| invocation_retirable_after_closure(s, &member_set, i, at))
}

fn demand_retirable(s: &Document, d: &Demand) -> bool {
    d.process.is_none()
        && !d.effect_started
        && s.claims.values().all(|owner| owner != &d.id)
        && matches!(
            d.state,
            DemandState::Released
                | DemandState::Cancelled
                | DemandState::Expired
                | DemandState::Preempted
                | DemandState::Unavailable
        )
}

fn invocation_retirable_after_closure(
    s: &Document,
    member_set: &BTreeSet<String>,
    invocation: &Invocation,
    at: u64,
) -> bool {
    match invocation.state {
        InvocationState::Queued | InvocationState::Running => return false,
        InvocationState::Uncertain
            if !physical_cleanup_proven(s, &invocation.request.reservation_id) =>
        {
            return false;
        }
        _ => {}
    }
    workspace_dependencies_retirable(s, member_set, invocation, at)
}

fn active_terminal_invocations(s: &Document, at: u64) -> Vec<String> {
    s.data
        .invocations
        .values()
        .filter(|i| {
            matches!(
                i.state,
                InvocationState::Completed
                    | InvocationState::Cancelled
                    | InvocationState::Failed
                    | InvocationState::Expired
            ) && active_invocation_dependencies_clear(s, i, at)
        })
        .map(|i| i.id.clone())
        .collect()
}

fn active_invocation_dependencies_clear(s: &Document, invocation: &Invocation, at: u64) -> bool {
    let member_set = BTreeSet::from([invocation.request.reservation_id.clone()]);
    workspace_dependencies_retirable(s, &member_set, invocation, at)
}

fn workspace_dependencies_retirable(
    s: &Document,
    member_set: &BTreeSet<String>,
    invocation: &Invocation,
    at: u64,
) -> bool {
    if let Some(scope_id) = invocation.request.workspace_scope.as_ref() {
        if let Some(scope) = s.data.workspace_scopes.get(scope_id) {
            if scope_open(scope, at) {
                return false;
            }
            if scope.invocation_id.as_ref().is_some_and(|parent| {
                s.data.invocations.get(parent).is_some_and(|i| {
                    matches!(
                        i.state,
                        InvocationState::Queued
                            | InvocationState::Running
                            | InvocationState::Uncertain
                    )
                })
            }) {
                return false;
            }
        }
    }
    if crate::work_payload::is_workspace(invocation) {
        return !s.data.invocations.values().any(|child| {
            child
                .request
                .workspace_scope
                .as_ref()
                .and_then(|scope| s.data.workspace_scopes.get(scope))
                .and_then(|scope| scope.invocation_id.as_ref())
                == Some(&invocation.id)
                && member_set.contains(&child.request.reservation_id)
                && matches!(
                    child.state,
                    InvocationState::Queued | InvocationState::Running
                )
        });
    }
    true
}

fn scopes_closed_or_expired(s: &Document, member_set: &BTreeSet<String>, at: u64) -> bool {
    s.data
        .workspace_scopes
        .values()
        .filter(|scope| member_set.contains(&scope.reservation_id))
        .all(|scope| !scope_open(scope, at))
}

fn scope_open(scope: &crate::workspace_scope::WorkspaceScope, at: u64) -> bool {
    scope.closed_at_ms.is_none() && scope.deadline_ms > at.saturating_mul(1000)
}

fn physical_cleanup_proven(s: &Document, reservation_id: &str) -> bool {
    s.data
        .demands
        .get(reservation_id)
        .is_some_and(|d| d.recovery_reconciliation.is_some())
}

#[allow(dead_code)]
fn retire_closed_group(
    root: &Path,
    s: &mut Document,
    root_id: &str,
    at: u64,
    report: &mut MaintenanceReport,
) -> Result<(), String> {
    let root_demand = s
        .data
        .demands
        .get(root_id)
        .cloned()
        .ok_or("missing_retirement_root")?;
    let member_ids = member_ids(s, &root_demand).ok_or("missing_retirement_member")?;
    let member_set = member_ids.iter().cloned().collect::<BTreeSet<_>>();
    let invocations = s
        .data
        .invocations
        .values()
        .filter(|i| member_set.contains(&i.request.reservation_id))
        .cloned()
        .collect::<Vec<_>>();
    for invocation in &invocations {
        archive_invocation(root, s, invocation, Some(at))?;
    }
    write_closed_reservation(root, s, &root_demand, &member_ids, at)?;
    for invocation in invocations {
        s.data.invocations.remove(&invocation.id);
        report.archived_invocations += 1;
    }
    s.data
        .workspace_scopes
        .retain(|_, scope| !member_set.contains(&scope.reservation_id));
    for id in member_ids {
        s.data.demands.remove(&id);
    }
    report.archived_reservations += 1;
    Ok(())
}

fn member_ids(s: &Document, root: &Demand) -> Option<Vec<String>> {
    if root.services.is_empty() {
        return Some(vec![root.id.clone()]);
    }
    let ids = root.services.values().cloned().collect::<Vec<_>>();
    ids.iter()
        .all(|id| s.data.demands.contains_key(id))
        .then_some(ids)
}

#[allow(dead_code)]
fn archive_invocation(
    root: &Path,
    s: &Document,
    invocation: &Invocation,
    closed_at: Option<u64>,
) -> Result<(), String> {
    let path = invocation_path(root, &invocation.owner, &invocation.id);
    let existing: Option<InvocationArchive> = read_json(&path)?;
    let mut archive = if let Some(existing) = existing {
        existing
    } else {
        let mut record = invocation.clone();
        if !matches!(
            record.state,
            InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
        ) {
            crate::capacity::compact_terminal_request(&mut record)?;
        }
        InvocationArchive {
            schema: ARCHIVE_SCHEMA.into(),
            owner: invocation.owner.clone(),
            invocation_id: invocation.id.clone(),
            reservation_id: invocation.request.reservation_id.clone(),
            submission_id: invocation.request.submission_id.clone(),
            work_id: invocation.request.work.as_ref().map(|w| w.work_id.clone()),
            closed_at: None,
            closed_at_source: None,
            expires_at: None,
            preserved: false,
            record,
        }
    };
    if archive.owner != invocation.owner || archive.invocation_id != invocation.id {
        return Err("archive_invocation_identity_conflict".into());
    }
    apply_closure(
        &mut archive.closed_at,
        &mut archive.closed_at_source,
        &mut archive.expires_at,
        archive.preserved,
        closed_at,
    );
    write_json(&path, &archive)?;
    write_invocation_indexes(root, &archive)?;
    update_reservation_invocation_index(root, s, &archive)?;
    Ok(())
}

#[allow(dead_code)]
fn write_invocation_indexes(root: &Path, archive: &InvocationArchive) -> Result<(), String> {
    let pointer = InvocationPointer {
        schema: ARCHIVE_SCHEMA.into(),
        owner: archive.owner.clone(),
        invocation_id: archive.invocation_id.clone(),
        expires_at: archive.expires_at,
    };
    write_json(
        &submission_index_path(
            root,
            &archive.owner,
            &archive.reservation_id,
            &archive.submission_id,
        ),
        &pointer,
    )?;
    if let Some(work_id) = archive.work_id.as_ref() {
        write_json(&work_index_path(root, &archive.owner, work_id), &pointer)?;
    }
    Ok(())
}

#[allow(dead_code)]
fn update_reservation_invocation_index(
    root: &Path,
    s: &Document,
    archive: &InvocationArchive,
) -> Result<(), String> {
    let path = reservation_path(root, &archive.owner, &archive.reservation_id);
    let mut reservation =
        read_json(&path)?.unwrap_or_else(|| active_reservation_archive(s, archive));
    append_unique(
        &mut reservation.invocation_ids,
        archive.invocation_id.clone(),
    );
    if archive.closed_at.is_some() {
        apply_closure(
            &mut reservation.closed_at,
            &mut reservation.closed_at_source,
            &mut reservation.expires_at,
            reservation.preserved,
            archive.closed_at,
        );
    }
    write_reservation_archive(root, &reservation)
}

#[allow(dead_code)]
fn active_reservation_archive(s: &Document, archive: &InvocationArchive) -> ReservationArchive {
    let demand = s.data.demands.get(&archive.reservation_id);
    ReservationArchive {
        schema: ARCHIVE_SCHEMA.into(),
        owner: archive.owner.clone(),
        reservation_id: archive.reservation_id.clone(),
        acquisition_id: demand
            .and_then(|d| d.reserve_request.as_ref())
            .map(|r| r.acquisition_id.clone()),
        request: demand.and_then(|d| d.reserve_request.clone()),
        result: demand.and_then(|d| d.reserve_result.clone()),
        closed_at: None,
        closed_at_source: None,
        expires_at: None,
        preserved: false,
        demands: demand.into_iter().cloned().collect(),
        invocation_ids: Vec::new(),
        workspace_scopes: BTreeMap::new(),
    }
}

#[allow(dead_code)]
fn write_closed_reservation(
    root: &Path,
    s: &Document,
    root_demand: &Demand,
    member_ids: &[String],
    closed_at: u64,
) -> Result<(), String> {
    let path = reservation_path(root, &root_demand.owner, &root_demand.id);
    let existing: Option<ReservationArchive> = read_json(&path)?;
    let mut invocation_ids = existing
        .as_ref()
        .map(|a| a.invocation_ids.clone())
        .unwrap_or_default();
    for invocation in s.data.invocations.values() {
        if member_ids.contains(&invocation.request.reservation_id) {
            append_unique(&mut invocation_ids, invocation.id.clone());
        }
    }
    let member_set = member_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut archive = ReservationArchive {
        schema: ARCHIVE_SCHEMA.into(),
        owner: root_demand.owner.clone(),
        reservation_id: root_demand.id.clone(),
        acquisition_id: root_demand
            .reserve_request
            .as_ref()
            .map(|r| r.acquisition_id.clone()),
        request: root_demand.reserve_request.clone(),
        result: root_demand.reserve_result.clone(),
        closed_at: existing.as_ref().and_then(|a| a.closed_at),
        closed_at_source: existing.as_ref().and_then(|a| a.closed_at_source.clone()),
        expires_at: existing.as_ref().and_then(|a| a.expires_at),
        preserved: existing.as_ref().is_some_and(|a| a.preserved),
        demands: member_ids
            .iter()
            .filter_map(|id| s.data.demands.get(id).cloned())
            .collect(),
        invocation_ids,
        workspace_scopes: s
            .data
            .workspace_scopes
            .iter()
            .filter(|(_, scope)| member_set.contains(&scope.reservation_id))
            .map(|(id, scope)| (id.clone(), scope.clone()))
            .collect(),
    };
    apply_closure(
        &mut archive.closed_at,
        &mut archive.closed_at_source,
        &mut archive.expires_at,
        archive.preserved,
        Some(closed_at),
    );
    write_reservation_archive(root, &archive)?;
    for id in &archive.invocation_ids {
        if let Some(mut invocation) = read_invocation(root, &archive.owner, id)? {
            apply_closure(
                &mut invocation.closed_at,
                &mut invocation.closed_at_source,
                &mut invocation.expires_at,
                invocation.preserved,
                archive.closed_at,
            );
            write_json(&invocation_path(root, &archive.owner, id), &invocation)?;
            write_invocation_indexes(root, &invocation)?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn write_reservation_archive(root: &Path, archive: &ReservationArchive) -> Result<(), String> {
    write_json(
        &reservation_path(root, &archive.owner, &archive.reservation_id),
        archive,
    )?;
    if let Some(acquisition_id) = archive.acquisition_id.as_ref() {
        let pointer = ReservationPointer {
            schema: ARCHIVE_SCHEMA.into(),
            owner: archive.owner.clone(),
            reservation_id: archive.reservation_id.clone(),
            expires_at: archive.expires_at,
        };
        write_json(
            &acquisition_index_path(root, &archive.owner, acquisition_id),
            &pointer,
        )?;
    }
    Ok(())
}

fn apply_closure(
    closed_at: &mut Option<u64>,
    source: &mut Option<String>,
    expires_at: &mut Option<u64>,
    preserved: bool,
    candidate: Option<u64>,
) {
    if closed_at.is_none() {
        if let Some(value) = candidate {
            *closed_at = Some(value);
            *source = Some("verified_at_retirement".into());
        }
    }
    if !preserved && expires_at.is_none() {
        *expires_at = closed_at.map(|value| value.saturating_add(RETENTION_SECONDS));
    }
}

fn stage_invocation_archive(
    root: &Path,
    stage_id: &str,
    invocation: &Invocation,
    reservation: Option<&Demand>,
    closed_at: Option<u64>,
    stage_reservation_index: bool,
    staged: &mut Vec<StagedFile>,
) -> Result<InvocationArchive, String> {
    let archive = invocation_archive_from_snapshot(root, invocation, closed_at)?;
    stage_invocation_archive_value(root, stage_id, &archive, staged)?;
    stage_invocation_indexes(root, stage_id, &archive, staged)?;
    if stage_reservation_index {
        stage_reservation_invocation_index(root, stage_id, reservation, &archive, staged)?;
    }
    Ok(archive)
}

fn invocation_archive_from_snapshot(
    root: &Path,
    invocation: &Invocation,
    closed_at: Option<u64>,
) -> Result<InvocationArchive, String> {
    let mut archive =
        if let Some(existing) = read_invocation(root, &invocation.owner, &invocation.id)? {
            if existing.owner != invocation.owner || existing.invocation_id != invocation.id {
                return Err("archive_invocation_identity_conflict".into());
            }
            existing
        } else {
            let mut record = invocation.clone();
            if !matches!(
                record.state,
                InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
            ) {
                crate::capacity::compact_terminal_request(&mut record)?;
            }
            InvocationArchive {
                schema: ARCHIVE_SCHEMA.into(),
                owner: invocation.owner.clone(),
                invocation_id: invocation.id.clone(),
                reservation_id: invocation.request.reservation_id.clone(),
                submission_id: invocation.request.submission_id.clone(),
                work_id: invocation.request.work.as_ref().map(|w| w.work_id.clone()),
                closed_at: None,
                closed_at_source: None,
                expires_at: None,
                preserved: false,
                record,
            }
        };
    apply_closure(
        &mut archive.closed_at,
        &mut archive.closed_at_source,
        &mut archive.expires_at,
        archive.preserved,
        closed_at,
    );
    Ok(archive)
}

fn stage_invocation_archive_value(
    root: &Path,
    stage_id: &str,
    archive: &InvocationArchive,
    staged: &mut Vec<StagedFile>,
) -> Result<(), String> {
    stage_json(
        root,
        stage_id,
        &invocation_path(root, &archive.owner, &archive.invocation_id),
        archive,
        staged,
    )
}

fn stage_invocation_indexes(
    root: &Path,
    stage_id: &str,
    archive: &InvocationArchive,
    staged: &mut Vec<StagedFile>,
) -> Result<(), String> {
    let pointer = InvocationPointer {
        schema: ARCHIVE_SCHEMA.into(),
        owner: archive.owner.clone(),
        invocation_id: archive.invocation_id.clone(),
        expires_at: archive.expires_at,
    };
    stage_json(
        root,
        stage_id,
        &submission_index_path(
            root,
            &archive.owner,
            &archive.reservation_id,
            &archive.submission_id,
        ),
        &pointer,
        staged,
    )?;
    if let Some(work_id) = archive.work_id.as_ref() {
        stage_json(
            root,
            stage_id,
            &work_index_path(root, &archive.owner, work_id),
            &pointer,
            staged,
        )?;
    }
    Ok(())
}

fn stage_reservation_invocation_index(
    root: &Path,
    stage_id: &str,
    demand: Option<&Demand>,
    archive: &InvocationArchive,
    staged: &mut Vec<StagedFile>,
) -> Result<(), String> {
    let path = reservation_path(root, &archive.owner, &archive.reservation_id);
    let mut reservation = read_json(&path)?
        .unwrap_or_else(|| active_reservation_archive_from_snapshot(archive, demand));
    append_unique(
        &mut reservation.invocation_ids,
        archive.invocation_id.clone(),
    );
    if archive.closed_at.is_some() {
        apply_closure(
            &mut reservation.closed_at,
            &mut reservation.closed_at_source,
            &mut reservation.expires_at,
            reservation.preserved,
            archive.closed_at,
        );
    }
    stage_reservation_archive(root, stage_id, &reservation, staged)
}

fn active_reservation_archive_from_snapshot(
    archive: &InvocationArchive,
    demand: Option<&Demand>,
) -> ReservationArchive {
    ReservationArchive {
        schema: ARCHIVE_SCHEMA.into(),
        owner: archive.owner.clone(),
        reservation_id: archive.reservation_id.clone(),
        acquisition_id: demand
            .and_then(|d| d.reserve_request.as_ref())
            .map(|r| r.acquisition_id.clone()),
        request: demand.and_then(|d| d.reserve_request.clone()),
        result: demand.and_then(|d| d.reserve_result.clone()),
        closed_at: None,
        closed_at_source: None,
        expires_at: None,
        preserved: false,
        demands: demand.into_iter().cloned().collect(),
        invocation_ids: Vec::new(),
        workspace_scopes: BTreeMap::new(),
    }
}

fn closed_reservation_archive_from_snapshot(
    root: &Path,
    root_demand: &Demand,
    _member_ids: &[String],
    members: &[Demand],
    invocations: &[Invocation],
    workspace_scopes: &BTreeMap<String, crate::workspace_scope::WorkspaceScope>,
    member_set: &BTreeSet<String>,
    closed_at: u64,
) -> Result<ReservationArchive, String> {
    let path = reservation_path(root, &root_demand.owner, &root_demand.id);
    let existing: Option<ReservationArchive> = read_json(&path)?;
    let mut invocation_ids = existing
        .as_ref()
        .map(|a| a.invocation_ids.clone())
        .unwrap_or_default();
    for invocation in invocations {
        if member_set.contains(&invocation.request.reservation_id) {
            append_unique(&mut invocation_ids, invocation.id.clone());
        }
    }
    let mut archive = ReservationArchive {
        schema: ARCHIVE_SCHEMA.into(),
        owner: root_demand.owner.clone(),
        reservation_id: root_demand.id.clone(),
        acquisition_id: root_demand
            .reserve_request
            .as_ref()
            .map(|r| r.acquisition_id.clone()),
        request: root_demand.reserve_request.clone(),
        result: root_demand.reserve_result.clone(),
        closed_at: existing.as_ref().and_then(|a| a.closed_at),
        closed_at_source: existing.as_ref().and_then(|a| a.closed_at_source.clone()),
        expires_at: existing.as_ref().and_then(|a| a.expires_at),
        preserved: existing.as_ref().is_some_and(|a| a.preserved),
        demands: members.to_vec(),
        invocation_ids,
        workspace_scopes: workspace_scopes.clone(),
    };
    apply_closure(
        &mut archive.closed_at,
        &mut archive.closed_at_source,
        &mut archive.expires_at,
        archive.preserved,
        Some(closed_at),
    );
    Ok(archive)
}

fn stage_reservation_archive(
    root: &Path,
    stage_id: &str,
    archive: &ReservationArchive,
    staged: &mut Vec<StagedFile>,
) -> Result<(), String> {
    stage_json(
        root,
        stage_id,
        &reservation_path(root, &archive.owner, &archive.reservation_id),
        archive,
        staged,
    )?;
    if let Some(acquisition_id) = archive.acquisition_id.as_ref() {
        let pointer = ReservationPointer {
            schema: ARCHIVE_SCHEMA.into(),
            owner: archive.owner.clone(),
            reservation_id: archive.reservation_id.clone(),
            expires_at: archive.expires_at,
        };
        stage_json(
            root,
            stage_id,
            &acquisition_index_path(root, &archive.owner, acquisition_id),
            &pointer,
            staged,
        )?;
    }
    Ok(())
}

fn stage_json<T: Serialize>(
    root: &Path,
    stage_id: &str,
    final_path: &Path,
    value: &T,
    staged: &mut Vec<StagedFile>,
) -> Result<(), String> {
    let index = staged.len();
    let path_digest = digest(final_path.display().to_string().as_bytes());
    let stage_path = root
        .join("archive")
        .join("staging")
        .join(stage_id)
        .join(format!("{index:04}-{path_digest}.json"));
    let base_contents = match fs::read_to_string(final_path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    write_json(&stage_path, value)?;
    staged.push(StagedFile {
        final_path: final_path.to_path_buf(),
        stage_path,
        base_contents,
    });
    Ok(())
}

fn publish_staged(staged: &[StagedFile]) -> Result<(), String> {
    pause_maintenance_publish_io_if_configured();
    for file in staged {
        let contents = fs::read_to_string(&file.stage_path).map_err(|e| e.to_string())?;
        match fs::read_to_string(&file.final_path) {
            Ok(existing) if existing == contents => {
                let _ = fs::remove_file(&file.stage_path);
            }
            Ok(existing) if file.base_contents.as_ref() == Some(&existing) => {
                atomic_write(&file.final_path, &contents)?;
                let _ = fs::remove_file(&file.stage_path);
            }
            Ok(_) => {
                return Err("history_archive_publish_conflict".into());
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && file.base_contents.is_none() =>
            {
                atomic_write(&file.final_path, &contents)?;
                let _ = fs::remove_file(&file.stage_path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err("history_archive_publish_conflict".into());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn discard_staged(staged: &[StagedFile]) {
    for file in staged {
        let _ = fs::remove_file(&file.stage_path);
    }
}

fn serialized_equal<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

#[cfg(test)]
pub(crate) struct MaintenanceIoPause {
    entered: (std::sync::Mutex<bool>, std::sync::Condvar),
    release: (std::sync::Mutex<bool>, std::sync::Condvar),
}

#[cfg(test)]
impl MaintenanceIoPause {
    pub(crate) fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            entered: (std::sync::Mutex::new(false), std::sync::Condvar::new()),
            release: (std::sync::Mutex::new(false), std::sync::Condvar::new()),
        })
    }

    pub(crate) fn wait_until_entered(&self) {
        let (lock, condvar) = &self.entered;
        let mut entered = lock.lock().unwrap();
        while !*entered {
            entered = condvar.wait(entered).unwrap();
        }
    }

    pub(crate) fn release(&self) {
        let (lock, condvar) = &self.release;
        *lock.lock().unwrap() = true;
        condvar.notify_all();
    }
}

#[cfg(test)]
static MAINTENANCE_IO_PAUSE: std::sync::Mutex<Option<std::sync::Arc<MaintenanceIoPause>>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
static MAINTENANCE_PUBLISH_IO_PAUSE: std::sync::Mutex<Option<std::sync::Arc<MaintenanceIoPause>>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn install_maintenance_io_pause(pause: std::sync::Arc<MaintenanceIoPause>) {
    *MAINTENANCE_IO_PAUSE.lock().unwrap() = Some(pause);
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn clear_maintenance_io_pause() {
    *MAINTENANCE_IO_PAUSE.lock().unwrap() = None;
}

#[cfg(test)]
pub(crate) fn install_maintenance_publish_io_pause(pause: std::sync::Arc<MaintenanceIoPause>) {
    *MAINTENANCE_PUBLISH_IO_PAUSE.lock().unwrap() = Some(pause);
}

#[cfg(test)]
pub(crate) fn clear_maintenance_publish_io_pause() {
    *MAINTENANCE_PUBLISH_IO_PAUSE.lock().unwrap() = None;
}

#[cfg(test)]
fn pause_for_test(pause: Option<std::sync::Arc<MaintenanceIoPause>>) {
    if let Some(pause) = pause {
        let (entered_lock, entered_condvar) = &pause.entered;
        *entered_lock.lock().unwrap() = true;
        entered_condvar.notify_all();
        let (release_lock, release_condvar) = &pause.release;
        let mut released = release_lock.lock().unwrap();
        while !*released {
            released = release_condvar.wait(released).unwrap();
        }
    }
}

#[cfg(test)]
fn pause_maintenance_io_if_configured() {
    let pause = MAINTENANCE_IO_PAUSE.lock().unwrap().take();
    pause_for_test(pause);
}

#[cfg(test)]
fn pause_maintenance_publish_io_if_configured() {
    let pause = MAINTENANCE_PUBLISH_IO_PAUSE.lock().unwrap().take();
    pause_for_test(pause);
}

#[cfg(not(test))]
fn pause_maintenance_io_if_configured() {}

#[cfg(not(test))]
fn pause_maintenance_publish_io_if_configured() {}

fn expire(root: &Path, at: u64) -> Result<usize, String> {
    let mut removed = 0;
    for owner in owner_dirs(root)? {
        for archive in invocation_archives(&owner)? {
            if archive.preserved || !expired(archive.expires_at, at) {
                continue;
            }
            crate::payload_store::remove_invocation_payloads(root, &archive.record)?;
            remove_file_if_exists(&invocation_path(
                root,
                &archive.owner,
                &archive.invocation_id,
            ))?;
            remove_file_if_exists(&submission_index_path(
                root,
                &archive.owner,
                &archive.reservation_id,
                &archive.submission_id,
            ))?;
            if let Some(work_id) = archive.work_id.as_ref() {
                remove_file_if_exists(&work_index_path(root, &archive.owner, work_id))?;
            }
            removed += 1;
        }
        for archive in reservation_archives(&owner)? {
            if archive.preserved || !expired(archive.expires_at, at) {
                continue;
            }
            remove_file_if_exists(&reservation_path(
                root,
                &archive.owner,
                &archive.reservation_id,
            ))?;
            if let Some(acquisition_id) = archive.acquisition_id.as_ref() {
                remove_file_if_exists(&acquisition_index_path(
                    root,
                    &archive.owner,
                    acquisition_id,
                ))?;
            }
            removed += 1;
        }
    }
    Ok(removed)
}

fn expired(expires_at: Option<u64>, at: u64) -> bool {
    expires_at.is_some_and(|expiry| expiry <= at)
}

#[allow(dead_code)]
fn expired_archive_exists(root: &Path, at: u64) -> Result<bool, String> {
    for owner in owner_dirs(root)? {
        if invocation_archives(&owner)?
            .into_iter()
            .any(|archive| !archive.preserved && expired(archive.expires_at, at))
            || reservation_archives(&owner)?
                .into_iter()
                .any(|archive| !archive.preserved && expired(archive.expires_at, at))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn append_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|saved| saved == &value) {
        values.push(value);
        values.sort();
    }
}

fn owner_dirs(root: &Path) -> Result<Vec<PathBuf>, String> {
    let path = root.join("archive").join("owners");
    let Ok(entries) = fs::read_dir(path) else {
        return Ok(Vec::new());
    };
    entries
        .map(|entry| entry.map(|entry| entry.path()).map_err(|e| e.to_string()))
        .collect()
}

fn invocation_archives(owner_dir: &Path) -> Result<Vec<InvocationArchive>, String> {
    archives_in(owner_dir.join("invocations"))
}

fn reservation_archives(owner_dir: &Path) -> Result<Vec<ReservationArchive>, String> {
    archives_in(owner_dir.join("reservations"))
}

fn candidate_reservation_archives(
    root: &Path,
    owner: &str,
) -> Result<Vec<ReservationArchive>, String> {
    reservation_archives(&owner_dir(root, owner))
}

fn archives_in<T: DeserializeOwned>(path: PathBuf) -> Result<Vec<T>, String> {
    let Ok(entries) = fs::read_dir(path) else {
        return Ok(Vec::new());
    };
    let mut archives = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(archive) = read_json(&path)? {
                archives.push(archive);
            }
        }
    }
    Ok(archives)
}

fn read_invocation(
    root: &Path,
    owner: &str,
    id: &str,
) -> Result<Option<InvocationArchive>, String> {
    read_json(&invocation_path(root, owner, id))
}

fn read_reservation(
    root: &Path,
    owner: &str,
    id: &str,
) -> Result<Option<ReservationArchive>, String> {
    read_json(&reservation_path(root, owner, id))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let contents = serde_json::to_string(value).map_err(|e| e.to_string())?;
    atomic_write(path, &contents)
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn owner_dir(root: &Path, owner: &str) -> PathBuf {
    root.join("archive")
        .join("owners")
        .join(digest(owner.as_bytes()))
}

fn invocation_path(root: &Path, owner: &str, id: &str) -> PathBuf {
    owner_dir(root, owner)
        .join("invocations")
        .join(format!("{}.json", digest(id.as_bytes())))
}

fn reservation_path(root: &Path, owner: &str, id: &str) -> PathBuf {
    owner_dir(root, owner)
        .join("reservations")
        .join(format!("{}.json", digest(id.as_bytes())))
}

fn submission_index_path(
    root: &Path,
    owner: &str,
    reservation_id: &str,
    submission_id: &str,
) -> PathBuf {
    owner_dir(root, owner)
        .join("submission-index")
        .join(format!(
            "{}.json",
            digest(format!("{reservation_id}\0{submission_id}").as_bytes())
        ))
}

fn work_index_path(root: &Path, owner: &str, work_id: &str) -> PathBuf {
    owner_dir(root, owner)
        .join("work-index")
        .join(format!("{}.json", digest(work_id.as_bytes())))
}

fn acquisition_index_path(root: &Path, owner: &str, acquisition_id: &str) -> PathBuf {
    owner_dir(root, owner)
        .join("acquisition-index")
        .join(format!("{}.json", digest(acquisition_id.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Backend, Driver, Profile};
    use rack_ai_application::GenericPriority;
    use serde_json::json;
    use std::{collections::BTreeMap, fs};

    fn root(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("rack-history-{name}-{}", identity().unwrap()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn profile() -> Profile {
        Profile {
            tag: "local-primary".into(),
            version: "test".into(),
            model: "model".into(),
            backend: Backend::Vllm,
            driver: Driver::Fixture,
            qualified: true,
            evidence: vec!["test".into()],
            capabilities: Vec::new(),
            context_tokens: 4096,
            max_input_tokens: 3968,
            protocols: crate::protocol::default_protocols(),
            streaming: false,
            max_images_per_request: 0,
            max_image_bytes: 0,
            max_image_pixels: 0,
            max_output_tokens: 128,
            resources: vec!["gpu".into()],
            device_mib: BTreeMap::from([("gpu".into(), 16)]),
            host_mib: 16,
            cpu_percent: 100,
            endpoint: "http://127.0.0.1:1".into(),
            executable: PathBuf::from("/bin/true"),
            container_image: None,
            container_mounts: BTreeMap::new(),
            executable_sha256: "sha".into(),
            args: Vec::new(),
            media_config: None,
            media_config_sha256: None,
            media_mode: None,
            artifact: None,
            artifact_sha256: None,
            artifact_verify_seconds: 1,
            startup_seconds: 1,
            drain_seconds: 1,
            stop_seconds: 1,
            inference_seconds: 10,
        }
    }

    fn acquire(id: &str) -> Acquire {
        Acquire {
            schema: VERSION.into(),
            source_system: "athba".into(),
            work_id: "work".into(),
            acquisition_id: format!("acquire-{id}"),
            tag: "local-primary".into(),
            priority: Some(GenericPriority::Low),
            capabilities: Vec::new(),
            context_tokens: 4096,
            ttl_seconds: 60,
            qualification: false,
        }
    }

    fn demand(id: &str, state: DemandState) -> Demand {
        Demand {
            reservation_id: Some(id.into()),
            services: BTreeMap::new(),
            reserve_request: None,
            reserve_result: None,
            reservation_closed: Some(state),
            recovery_reconciliation: None,
            recovery_error: None,
            workspace_recovery_analyses: BTreeMap::new(),
            id: id.into(),
            owner: "athba".into(),
            request: acquire(id),
            priority: GenericPriority::Low,
            profile: profile(),
            profile_hash: "profile".into(),
            state,
            reason: Some("released".into()),
            retry_after: None,
            preempted_by: None,
            ready_checked: true,
            accepted_calls: 1,
            generation: "generation".into(),
            access_key: "access".into(),
            backend_activation: None,
            created: 1,
            last_activity_at: None,
            order: 1,
            deadline: 100,
            transition_deadline: 100,
            victims: Vec::new(),
            process: None,
            effect_started: false,
            preflight_done: true,
            released: true,
        }
    }

    fn invocation(id: &str, reservation: &str, state: InvocationState) -> Invocation {
        Invocation {
            scope_access_hash: None,
            id: id.into(),
            owner: "athba".into(),
            request: Inference {
                work: None,
                schema: VERSION.into(),
                submission_id: format!("submission-{id}"),
                reservation_id: reservation.into(),
                generation: "generation".into(),
                profile_hash: "profile".into(),
                prompt: "hello".into(),
                payload: None,
                max_tokens: 16,
                timeout_seconds: 5,
                wait_seconds: None,
                workspace_scope: None,
            },
            request_digest: None,
            request_bytes: None,
            work_digest: None,
            work_bytes: None,
            request_ref: None,
            state,
            queue_order: 1,
            waiting_deadline: 100,
            execution_deadline: None,
            response_bytes: 1024,
            cancellation: None,
            late_result: None,
            result_digest: None,
            late_result_digest: None,
            result_ref: None,
            late_result_ref: None,
            started: Some(2),
            activation: Some("generation".into()),
            result: (state == InvocationState::Completed).then(|| json!({"model":"model"})),
            error: (state == InvocationState::Uncertain).then(|| "unknown".into()),
        }
    }

    fn document(demand: Demand, invocation: Invocation) -> Document {
        Document {
            claims: BTreeMap::new(),
            data: State {
                demands: BTreeMap::from([(demand.id.clone(), demand)]),
                invocations: BTreeMap::from([(invocation.id.clone(), invocation)]),
                ..State::default()
            },
        }
    }

    #[test]
    fn queued_and_running_invocations_remain_active() {
        for state in [InvocationState::Queued, InvocationState::Running] {
            let root = root("active");
            let mut doc = document(
                demand("reservation", DemandState::Released),
                invocation("call", "reservation", state),
            );
            assert_eq!(
                maintain(&root, &mut doc, 10).unwrap().archived_invocations,
                0
            );
            assert!(doc.data.invocations.contains_key("call"));
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn proven_absent_uncertainty_retires_without_rewriting_outcome() {
        let root = root("uncertain");
        let mut d = demand("reservation", DemandState::Released);
        d.recovery_reconciliation = Some(RecoveryReconciliation {
            historical_outcome: HistoricalOutcome::RecoveryOutcomeUnknown,
            current_effect: CurrentEffect::ProvenAbsent,
            reconciled_at: 9,
            cleanup_process: None,
            checks: RecoveryAbsenceChecks {
                reservation_inactive: true,
                active_invocations_absent: true,
                recorded_process_absent: true,
                systemd_activation_absent: true,
                gpu_allocation_absent: true,
                media_session_absent: true,
                lifecycle_transition_absent: true,
                ownership_fence_intact: true,
            },
        });
        let mut doc = document(
            d,
            invocation("uncertain", "reservation", InvocationState::Uncertain),
        );
        let report = maintain(&root, &mut doc, now()).unwrap();
        assert_eq!(report.archived_reservations, 1);
        assert!(doc.data.invocations.is_empty());
        let archived = lookup_invocation(&root, "athba", "uncertain")
            .unwrap()
            .unwrap();
        assert_eq!(archived.state, InvocationState::Uncertain);
        assert_eq!(archived.error.as_deref(), Some("unknown"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn archive_publication_conflict_leaves_active_records_intact() {
        let root = root("publish-conflict");
        let doc = document(
            demand("reservation", DemandState::Released),
            invocation("call", "reservation", InvocationState::Completed),
        );
        let plan = plan_maintenance(&root, &doc, 10).unwrap().unwrap();
        let prepared = prepare_maintenance(&root, plan).unwrap();
        assert!(prepared_maintenance_eligible(&doc, &prepared).unwrap());
        let final_path = prepared.staged[0].final_path.clone();
        fs::create_dir_all(final_path.parent().unwrap()).unwrap();
        fs::write(&final_path, r#"{"schema":"conflicting-newer-archive"}"#).unwrap();

        let error = publish_prepared_maintenance(&prepared).unwrap_err();
        assert_eq!(error, "history_archive_publish_conflict");
        assert!(doc.data.demands.contains_key("reservation"));
        assert!(doc.data.invocations.contains_key("call"));
        discard_prepared_maintenance(&prepared);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn archival_io_failure_leaves_active_records_intact() {
        let root = std::env::temp_dir().join(format!("rack-history-file-{}", identity().unwrap()));
        fs::write(&root, "not a directory").unwrap();
        let mut doc = document(
            demand("reservation", DemandState::Released),
            invocation("call", "reservation", InvocationState::Completed),
        );
        assert!(maintain(&root, &mut doc, 10).is_err());
        assert!(doc.data.demands.contains_key("reservation"));
        assert!(doc.data.invocations.contains_key("call"));
        fs::remove_file(root).unwrap();
    }
}
