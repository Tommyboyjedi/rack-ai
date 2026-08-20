use rack_ai_domain::ActiveNodeId;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskDagNode {
    id: ActiveNodeId,
    name: Option<String>,
    worker: String,
    cwd: String,
    prompt: String,
    #[serde(default)]
    depends_on: Vec<ActiveNodeId>,
    #[serde(default)]
    artifacts: Vec<Value>,
}

impl TaskDagNode {
    pub fn id(&self) -> &ActiveNodeId {
        &self.id
    }

    pub fn depends_on(&self) -> &[ActiveNodeId] {
        self.depends_on.as_slice()
    }

    pub fn execution_step(&self) -> Value {
        serde_json::json!({
            "name": self.name.clone().unwrap_or_else(|| self.id.value().to_string()),
            "worker": self.worker,
            "cwd": self.cwd,
            "prompt": self.prompt,
            "artifacts": self.artifacts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TaskDagNode;

    #[test]
    fn builds_execution_step_payload() {
        let node = serde_json::from_value::<TaskDagNode>(serde_json::json!({
            "id": "plan",
            "worker": "coder",
            "cwd": "/tmp/project",
            "prompt": "Plan this",
            "artifacts": ["plan.md"]
        }))
        .unwrap();
        let payload = node.execution_step();
        assert_eq!(payload["worker"], "coder");
        assert_eq!(payload["artifacts"][0], "plan.md");
    }
}
