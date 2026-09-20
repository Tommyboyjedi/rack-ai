use rack_ai_application::{
    AcceptanceDocument, ChangeRepositoryDocument, ChangeRequestDocument, GenericRoutingHeader,
    LimitsDocument, WorkspaceRequest, WorkspaceRequirements,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    pub registry_root: PathBuf,
    pub state_root: PathBuf,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub reservation_id: String,
    pub service: String,
    pub work_id: String,
    pub payload: Payload,
    #[serde(default)]
    pub wait_seconds: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Payload {
    Inference {
        prompt: String,
        max_tokens: u32,
        timeout_seconds: u64,
    },
    Workspace {
        workspace: Box<Workspace>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub repository: ChangeRepositoryDocument,
    pub objective: String,
    pub allowed_paths: Vec<String>,
    pub acceptance: AcceptanceDocument,
    #[serde(default)]
    pub requirements: WorkspaceRequirements,
    #[serde(default)]
    pub environment_resources: Vec<String>,
    pub limits: LimitsDocument,
}
impl Work {
    pub fn workspace(&self) -> Option<&Workspace> {
        match &self.payload {
            Payload::Workspace { workspace } => Some(workspace),
            _ => None,
        }
    }
    pub fn request(&self, d: &crate::types::Demand) -> Result<WorkspaceRequest, String> {
        let w = self.workspace().ok_or("workspace_required")?;
        let identity = format!(
            "work-{}",
            crate::types::digest(format!("{}/{}", d.owner, self.work_id).as_bytes())
        );
        Ok(WorkspaceRequest {
            change: ChangeRequestDocument {
                change_id: identity.clone(),
                repository: w.repository.clone(),
                task: w.objective.clone(),
                allowed_paths: w.allowed_paths.clone(),
                acceptance: w.acceptance.clone(),
                environment_resources: w.environment_resources.clone(),
                limits: w.limits.clone(),
            },
            requirements: w.requirements.clone(),
            routing: GenericRoutingHeader::new(
                d.owner.clone(),
                self.work_id.clone(),
                identity.clone(),
                identity,
                d.request.capabilities.clone(),
                d.priority,
            )?,
        })
    }
}

pub fn is_workspace(i: &crate::types::Invocation) -> bool {
    i.request
        .work
        .as_ref()
        .is_some_and(|w| w.workspace().is_some())
}

#[cfg(test)]
mod tests {
    use super::Work;
    use serde_json::json;

    fn workspace_payload(requirements: serde_json::Value) -> serde_json::Value {
        json!({
            "reservation_id": "reservation",
            "service": "local-coder",
            "work_id": "work",
            "payload": {
                "kind": "workspace",
                "workspace": {
                    "repository": {"id": "repo", "base_ref": "main"},
                    "objective": "Make one bounded change.",
                    "allowed_paths": ["src/"],
                    "acceptance": {"commands": [["cargo", "test"]]},
                    "requirements": requirements,
                    "limits": {"max_implementation_attempts": 1, "timeout_seconds": 30}
                }
            }
        })
    }

    #[test]
    fn workspace_payload_does_not_accept_client_context_window() {
        let ok = workspace_payload(json!({
            "complexity": "small",
            "requires_large_context": false
        }));
        assert!(serde_json::from_value::<Work>(ok).is_ok());

        let rejected = workspace_payload(json!({
            "complexity": "small",
            "requires_large_context": false,
            "context_window": 14_320
        }));
        assert!(serde_json::from_value::<Work>(rejected).is_err());
    }
}
