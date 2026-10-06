//! Optional reservation-scoped observability. Never grants execution authority.
use crate::{service::Service, types::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SCHEMA: &str = "rack-ai/model-interaction/v1";
pub const METADATA_BYTES: u64 = 8192;
pub const MAX_SWEEP: usize = 32;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub max_interactions_per_work: usize,
    pub max_interactions_per_reservation: usize,
    pub max_reservation_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_bytes: 65536,
            max_response_bytes: 65536,
            max_interactions_per_work: 64,
            max_interactions_per_reservation: 128,
            max_reservation_bytes: 4 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_request_bytes > 1024 * 1024
            || self.max_response_bytes > 1024 * 1024
            || self.max_interactions_per_work == 0
            || self.max_interactions_per_work > self.max_interactions_per_reservation
            || self.max_interactions_per_reservation > 1024
            || self.max_reservation_bytes
                < (self.max_interactions_per_reservation as u64 + 1) * METADATA_BYTES
            || self.max_reservation_bytes > 64 * 1024 * 1024
        {
            return Err("invalid interaction diagnostics limits".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CallIdentity {
    pub reservation_id: String,
    pub work_id: String,
    pub parent_invocation_id: String,
    pub invocation_id: String,
    pub sequence: u64,
    pub service: String,
    pub model: String,
    pub profile_version: String,
    pub profile_hash: String,
    pub protocol: crate::protocol::Protocol,
    pub accepted: Option<u64>,
    pub started: Option<u64>,
}

pub fn live(s: &Document, input: (&str, &str)) -> bool {
    let (owner, id) = input;
    let Some(root) = s.data.demands.get(id).filter(|d| d.owner == owner) else {
        return false;
    };
    if root.reservation_closed.is_some()
        || !root
            .reserve_request
            .as_ref()
            .is_some_and(|r| r.diagnostics.retained_model_interactions)
    {
        return false;
    }
    crate::reservation::members(s, root).is_ok_and(|ids| {
        ids.iter().any(|id| {
            s.data.demands.get(id).is_some_and(|d| {
                !matches!(
                    d.state,
                    DemandState::Released
                        | DemandState::Cancelled
                        | DemandState::Expired
                        | DemandState::Preempted
                        | DemandState::Unavailable
                )
            })
        })
    })
}

pub fn identity(
    service: &Service,
    input: (&Demand, &Invocation),
) -> Result<Option<CallIdentity>, String> {
    let (d, i) = input;
    if d.reservation_id.is_none()
        || d.reserve_request
            .as_ref()
            .is_some_and(|r| !r.diagnostics.retained_model_interactions)
    {
        return Ok(None);
    }
    let Some(scope_id) = i.request.workspace_scope.as_ref() else {
        return Ok(None);
    };
    service.authority.read(|s| {
        let root = crate::reservation::root(s, d)?;
        if !live(s, (&i.owner, &root.id)) {
            return Ok(None);
        }
        let scope = s
            .data
            .workspace_scopes
            .get(scope_id)
            .ok_or("missing_workspace_scope")?;
        let Some(parent_id) = scope.invocation_id.as_ref() else {
            return Ok(None);
        };
        let parent = s
            .data
            .invocations
            .get(parent_id)
            .ok_or("missing_parent_invocation")?;
        let Some(work) = parent.request.work.as_ref() else {
            return Ok(None);
        };
        Ok(Some(CallIdentity {
            reservation_id: root.id.clone(),
            work_id: work.work_id.clone(),
            parent_invocation_id: parent_id.clone(),
            invocation_id: i.id.clone(),
            sequence: i.queue_order,
            service: d.profile.tag.clone(),
            model: d.profile.model.clone(),
            profile_version: d.profile.version.clone(),
            profile_hash: d.profile_hash.clone(),
            protocol: i
                .request
                .payload
                .as_ref()
                .map_or(crate::protocol::Protocol::ChatCompletions, |p| p.protocol),
            accepted: (i.created > 0).then_some(i.created),
            started: i.started,
        }))
    })
}

pub fn supported() -> Value {
    json!({"supported":true,"schema":SCHEMA,"enablement":"reserve.diagnostics.retained_model_interactions",
        "retrieval_operation":"get_work_artifact","retention":"active_reservation_only"})
}

pub fn maintenance(service: &Service) {
    if let Err(error) = crate::interaction_store::sweep(service) {
        eprintln!(
            "interaction diagnostics cleanup pending: {}",
            crate::capacity::diagnostic(error)
        );
    }
}
