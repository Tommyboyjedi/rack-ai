use rack_ai_domain::ActiveNodeId;
use rack_ai_domain::DagNodeState;
use rack_ai_domain::DagRunState;
use rack_ai_domain::Placement;
use serde::Deserialize;
use serde::Serialize;

use crate::TaskDag;
use crate::WorkerCatalog;

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
        dag.validate()?;
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

    pub fn dag_node_placement(
        &self,
        node_id: &ActiveNodeId,
        worker_catalog: &dyn WorkerCatalog,
    ) -> Result<Placement, String> {
        let node = self.find_dag_node(node_id)?;
        let binding = worker_catalog.resolve(node.worker_id())?;
        Ok(binding.placement())
    }

    pub fn build_execution_spec_json(
        &self,
        node_id: &ActiveNodeId,
        placement: &Placement,
    ) -> Result<String, String> {
        let node = self.find_dag_node(node_id)?;
        let json = serde_json::json!({
            "task_id": format!("{}--{}", self.task_id, node_id.value()),
            "template": self.template.clone().unwrap_or_else(|| "dag-node".to_string()),
            "request": self.request,
            "placement": placement,
            "steps": [node.execution_step()],
        });
        serde_json::to_string_pretty(&json).map_err(|error| error.to_string())
    }

    pub fn has_dag(&self) -> bool {
        self.dag.is_some()
    }

    pub fn placement(&self) -> &Placement {
        &self.placement
    }

    fn find_dag_node(&self, node_id: &ActiveNodeId) -> Result<&crate::TaskDagNode, String> {
        let dag = self
            .dag
            .as_ref()
            .ok_or("task does not contain a dag".to_string())?;
        dag.find_node(node_id)
            .ok_or("dag node missing from task spec".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::TaskSpec;
    use crate::WorkerBinding;
    use crate::WorkerCatalog;

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
        let placement = spec
            .dag_node_placement(
                &rack_ai_domain::ActiveNodeId::new("plan".to_string()).unwrap(),
                &FakeWorkerCatalog,
            )
            .unwrap();
        let json = spec
            .build_execution_spec_json(
                &rack_ai_domain::ActiveNodeId::new("plan".to_string()).unwrap(),
                &placement,
            )
            .unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        assert_eq!(value["steps"][0]["worker"], "planner");
        assert_eq!(value["placement"]["resource_ids"][0], "gpu-planner");
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

    struct FakeWorkerCatalog;

    impl WorkerCatalog for FakeWorkerCatalog {
        fn resolve(&self, worker_id: &str) -> Result<WorkerBinding, String> {
            Ok(WorkerBinding::new(
                worker_id.to_string(),
                format!("gpu-{worker_id}"),
                format!("model-{worker_id}"),
                "jcode".to_string(),
            ))
        }
    }
}
