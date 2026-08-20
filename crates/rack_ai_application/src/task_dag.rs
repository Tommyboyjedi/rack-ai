use std::collections::BTreeSet;

use rack_ai_domain::ActiveNodeId;
use serde::Deserialize;
use serde::Serialize;

use crate::TaskDagNode;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskDag {
    nodes: Vec<TaskDagNode>,
}

impl TaskDag {
    pub fn node_ids(&self) -> Vec<ActiveNodeId> {
        self.nodes.iter().map(|node| node.id().clone()).collect()
    }

    pub fn find_node(&self, node_id: &ActiveNodeId) -> Option<&TaskDagNode> {
        self.nodes.iter().find(|node| node.id() == node_id)
    }

    pub fn nodes(&self) -> &[TaskDagNode] {
        self.nodes.as_slice()
    }

    pub fn validate(&self) -> Result<(), String> {
        let known_ids: BTreeSet<String> = self
            .nodes
            .iter()
            .map(|node| node.id().value().to_string())
            .collect();
        for node in &self.nodes {
            for dependency in node.depends_on() {
                if !known_ids.contains(dependency.value()) {
                    return Err(format!(
                        "dag node {} depends on unknown node {}",
                        node.id().value(),
                        dependency.value()
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TaskDag;

    #[test]
    fn finds_node_by_id() {
        let dag = serde_json::from_value::<TaskDag>(serde_json::json!({
            "nodes": [
                {"id": "plan", "worker": "planner", "cwd": "/tmp", "prompt": "Plan"},
                {"id": "code", "worker": "coder", "cwd": "/tmp", "prompt": "Code"}
            ]
        }))
        .unwrap();
        assert!(
            dag.find_node(&rack_ai_domain::ActiveNodeId::new("code".to_string()).unwrap())
                .is_some()
        );
    }

    #[test]
    fn rejects_unknown_dependencies() {
        let dag = serde_json::from_value::<TaskDag>(serde_json::json!({
            "nodes": [
                {"id": "verify", "worker": "planner", "cwd": "/tmp", "prompt": "Verify", "depends_on": ["missing"]}
            ]
        }))
        .unwrap();
        assert_eq!(
            dag.validate(),
            Err("dag node verify depends on unknown node missing".to_string())
        );
    }
}
