use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

use crate::ActiveNodeId;
use crate::DagNodeState;
use crate::DagNodeStatus;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DagRunState {
    nodes: BTreeMap<ActiveNodeId, DagNodeState>,
}

impl DagRunState {
    pub fn new(nodes: Vec<(ActiveNodeId, DagNodeState)>) -> Result<Self, String> {
        let mut map = BTreeMap::new();
        for (node_id, state) in nodes {
            if map.insert(node_id, state).is_some() {
                return Err("duplicate dag node id".to_string());
            }
        }
        Ok(Self { nodes: map })
    }

    pub fn all_succeeded(&self) -> bool {
        !self.nodes.is_empty()
            && self
                .nodes
                .values()
                .all(|state| state.status() == &DagNodeStatus::Succeeded)
    }

    pub fn node_state(&self, node_id: &ActiveNodeId) -> Option<&DagNodeState> {
        self.nodes.get(node_id)
    }

    pub fn ready_node_id_in_order(&self, node_ids: &[ActiveNodeId]) -> Option<ActiveNodeId> {
        node_ids.iter().find_map(|node_id| {
            let state = self.nodes.get(node_id)?;
            if state.status() != &DagNodeStatus::Pending {
                return None;
            }
            let is_ready = state.depends_on().iter().all(|dependency| {
                self.nodes
                    .get(dependency)
                    .map(|item| item.status() == &DagNodeStatus::Succeeded)
                    .unwrap_or(false)
            });
            if is_ready {
                return Some(node_id.clone());
            }
            None
        })
    }

    pub fn mark_running(&self, node_id: &ActiveNodeId, started_at: String) -> Result<Self, String> {
        self.map_node(node_id, |state| state.mark_running(started_at))
    }

    pub fn mark_pending(&self, node_id: &ActiveNodeId, last_error: String) -> Result<Self, String> {
        self.map_node(node_id, |state| state.mark_pending(last_error))
    }

    pub fn mark_succeeded(
        &self,
        node_id: &ActiveNodeId,
        finished_at: String,
        result_path: Option<String>,
    ) -> Result<Self, String> {
        self.map_node(node_id, |state| {
            state.mark_succeeded(finished_at, result_path)
        })
    }

    pub fn mark_failed(
        &self,
        node_id: &ActiveNodeId,
        finished_at: String,
        result_path: Option<String>,
        last_error: String,
    ) -> Result<Self, String> {
        self.map_node(node_id, |state| {
            state.mark_failed(finished_at, result_path, last_error)
        })
    }

    fn map_node<F>(&self, node_id: &ActiveNodeId, map: F) -> Result<Self, String>
    where
        F: FnOnce(DagNodeState) -> DagNodeState,
    {
        let mut nodes = self.nodes.clone();
        let state = nodes
            .remove(node_id)
            .ok_or("dag node state missing".to_string())?;
        nodes.insert(node_id.clone(), map(state));
        Ok(Self { nodes })
    }
}
