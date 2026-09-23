//! A registered workspace deadline is a durable dispatch fence, independent of HTTP lifetime.
use crate::{
    service::{Service, active},
    types::*,
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkspaceScope {
    #[serde(default)]
    pub invocation_id: Option<String>,
    pub owner: String,
    pub reservation_id: String,
    pub deadline_ms: u64,
    pub closed_at_ms: Option<u64>,
    authorization_hash: String,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeControl {
    Open {
        deadline_ms: u64,
        #[serde(default)]
        invocation_id: Option<String>,
    },
    Close,
}
pub struct ScopeAccess {
    pub reservation: String,
    pub capability: String,
    pub namespace: String,
}
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn key(reservation: &str, namespace: &str) -> String {
    digest(format!("{reservation}/{namespace}").as_bytes())
}
pub struct ScopeController<'a> {
    pub service: &'a Service,
}
impl ScopeController<'_> {
    pub fn control(&self, access: ScopeAccess, operation: ScopeControl) -> Result<(), String> {
        if !valid_id(&access.namespace) {
            return Err("invalid_call_namespace".into());
        }
        let id = key(&access.reservation, &access.namespace);
        let authorization_hash = digest(access.capability.as_bytes());
        self.service.authority.update(|s| {
            match operation {
                ScopeControl::Open {
                    deadline_ms,
                    invocation_id,
                } => {
                    let d = s
                        .data
                        .demands
                        .get(&access.reservation)
                        .ok_or("invalid_scoped_capability")?;
                    let current_key =
                        bool::from(d.access_key.as_bytes().ct_eq(access.capability.as_bytes()));
                    let accepted_runner = invocation_id.as_ref().is_some_and(|id| {
                        s.data.invocations.get(id).is_some_and(|i| {
                            parent_open(i)
                                && i.request.reservation_id == d.id
                                && i.owner == d.owner
                                && i.scope_access_hash.as_deref() == Some(&authorization_hash)
                        })
                    });
                    if !active(d)
                        || (!current_key && !accepted_runner)
                        || (invocation_id.is_some() && !accepted_runner)
                    {
                        return Err("invalid_scoped_capability".into());
                    }
                    if let Some(scope) = s.data.workspace_scopes.get(&id) {
                        return if scope.deadline_ms == deadline_ms
                            && scope.authorization_hash == authorization_hash
                            && scope.invocation_id == invocation_id
                        {
                            Ok(()) // Reconciliation never extends or reopens an execution.
                        } else {
                            Err("workspace_scope_identity_conflict".into())
                        };
                    }
                    // Match the bounded workspace TimeoutSeconds representation. Per-call
                    // waiting and execution limits remain independent and unchanged.
                    let remaining_bound = u64::from(u32::MAX);
                    if deadline_ms <= now_ms()
                        || deadline_ms > now_ms().saturating_add(remaining_bound * 1000)
                    {
                        return Err("workspace_scope_deadline_limits".into());
                    }
                    s.data.workspace_scopes.insert(
                        id,
                        WorkspaceScope {
                            invocation_id,
                            owner: d.owner.clone(),
                            reservation_id: d.id.clone(),
                            deadline_ms,
                            closed_at_ms: None,
                            authorization_hash,
                        },
                    );
                    crate::capacity::retention(s, &self.service.config.limits)?;
                }
                ScopeControl::Close => {
                    let scope = s
                        .data
                        .workspace_scopes
                        .get_mut(&id)
                        .ok_or("unknown_workspace_scope")?;
                    // The original capability may ONLY close its exact scope after rotation.
                    if !bool::from(
                        scope
                            .authorization_hash
                            .as_bytes()
                            .ct_eq(authorization_hash.as_bytes()),
                    ) {
                        return Err("invalid_scoped_capability".into());
                    }
                    scope.closed_at_ms.get_or_insert(now_ms());
                    cancel_closed(s);
                }
            }
            Ok(())
        })
    }
}
pub fn permits(s: &Document, request: &Inference) -> bool {
    request.workspace_scope.as_ref().is_none_or(|id| {
        s.data.workspace_scopes.get(id).is_some_and(|scope| {
            scope
                .invocation_id
                .as_ref()
                .is_none_or(|id| s.data.invocations.get(id).is_some_and(parent_open))
                && scope.reservation_id == request.reservation_id
                && scope.closed_at_ms.is_none()
                && scope.deadline_ms > now_ms()
        })
    })
}
pub fn needs_cancel(s: &Document, i: &Invocation) -> bool {
    i.cancellation.is_none()
        && matches!(
            i.state,
            InvocationState::Queued | InvocationState::Running | InvocationState::Uncertain
        )
        && !permits(s, &i.request)
}
pub fn cancel_closed(s: &mut Document) {
    let ids: Vec<_> = s
        .data
        .invocations
        .values()
        .filter(|i| needs_cancel(s, i))
        .map(|i| i.id.clone())
        .collect();
    for id in ids {
        if let Some(i) = s.data.invocations.get_mut(&id) {
            i.cancel();
        }
    }
}

fn parent_open(i: &Invocation) -> bool {
    crate::work_payload::is_workspace(i)
        && i.state == InvocationState::Running
        && i.cancellation.is_none()
        && i.execution_deadline
            .is_some_and(|deadline| deadline > now())
}
pub fn authorizes(s: &Document, input: (&str, &str, Option<&str>)) -> bool {
    let (reservation, capability, namespace) = input;
    namespace
        .and_then(|namespace| s.data.workspace_scopes.get(&key(reservation, namespace)))
        .is_some_and(|scope| {
            scope.reservation_id == reservation
                && scope.closed_at_ms.is_none()
                && scope.deadline_ms > now_ms()
                && scope
                    .invocation_id
                    .as_ref()
                    .is_none_or(|id| s.data.invocations.get(id).is_some_and(parent_open))
                && bool::from(
                    scope
                        .authorization_hash
                        .as_bytes()
                        .ct_eq(digest(capability.as_bytes()).as_bytes()),
                )
        })
}
