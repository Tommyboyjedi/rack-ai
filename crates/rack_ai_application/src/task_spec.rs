use rack_ai_domain::ActiveNodeId;
use rack_ai_domain::DagNodeState;
use rack_ai_domain::DagRunState;
use rack_ai_domain::Placement;
use serde::Deserialize;
use serde::Serialize;

use crate::TaskDag;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskSpec {
    task_id: String,
    placement: Placement,
    template: Option<String>,
    request: Option<String>,
    dag: Option<TaskDag>,
}

impl TaskSpec {
    pub fn dag_run_state(&self) -> Result<Option<DagRunState>, String> {
        let dag = match &self.dag {
            Some(value) => value,
            None => return Ok(None),
        };
        let states = dag
            .nodes()
            .iter()
            .map(|node| {
                (
                    node.id().clone(),
                    DagNodeState::pending(node.depends_on().to_vec()),
                )
            })
            .collect();
        Ok(Some(DagRunState::new(states)?))
    }

    pub fn first_ready_node_id(&self, dag_run_state: &DagRunState) -> Option<ActiveNodeId> {
        self.dag
            .as_ref()
            .and_then(|dag| dag_run_state.ready_node_id_in_order(&dag.node_ids()))
    }

    pub fn build_execution_spec_json(&self, node_id: &ActiveNodeId) -> Result<String, String> {
        let dag = self
            .dag
            .as_ref()
            .ok_or("task does not contain a dag".to_string())?;
        let node = dag
            .find_node(node_id)
            .ok_or("dag node missing from task spec".to_string())?;
        let json = serde_json::json!({
            "task_id": format!("{}--{}", self.task_id, node_id.value()),
            "template": self.template.clone().unwrap_or_else(|| "dag-node".to_string()),
            "request": self.request,
            "placement": self.placement,
            "steps": [node.execution_step()],
        });
        serde_json::to_string_pretty(&json).map_err(|error| error.to_string())
    }

    pub fn has_dag(&self) -> bool {
        self.dag.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::TaskSpec;

    #[test]
    fn builds_initial_dag_run_state() {
        let spec = sample_spec();
        let state = spec.dag_run_state().unwrap().unwrap();
        let ready = spec.first_ready_node_id(&state).unwrap();
        assert_eq!(ready.value(), "plan");
    }

    #[test]
    fn builds_single_node_execution_spec() {
        let spec = sample_spec();
        let json = spec
            .build_execution_spec_json(
                &rack_ai_domain::ActiveNodeId::new("plan".to_string()).unwrap(),
            )
            .unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        assert_eq!(value["steps"][0]["worker"], "planner");
    }

    fn sample_spec() -> TaskSpec {
        serde_json::from_value(serde_json::json!({
            "task_id": "task-1",
            "placement": {"worker_ids": ["planner", "coder"], "resource_ids": ["gpu0"], "model_ids": [], "backends": []},
            "dag": {
                "nodes": [
                    {"id": "plan", "worker": "planner", "cwd": "/tmp/project", "prompt": "Plan"},
                    {"id": "code", "worker": "coder", "cwd": "/tmp/project", "prompt": "Code", "depends_on": ["plan"]}
                ]
            }
        }))
        .unwrap()
    }
}
