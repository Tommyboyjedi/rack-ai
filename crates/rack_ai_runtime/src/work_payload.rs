use rack_ai_application::work_unit_request_document::*;
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
    pub repository: WorkUnitRepositoryDocument,
    pub objective: String,
    pub allowed_paths: Vec<String>,
    pub acceptance: WorkUnitAcceptanceDocument,
    #[serde(default)]
    pub requirements: WorkUnitRequirementsDocument,
    #[serde(default)]
    pub environment_resources: Vec<String>,
    pub limits: WorkUnitLimitsDocument,
}
impl Work {
    pub fn workspace(&self) -> Option<&Workspace> {
        match &self.payload {
            Payload::Workspace { workspace } => Some(workspace),
            _ => None,
        }
    }
    pub fn document(&self, d: &crate::types::Demand) -> Result<WorkUnitRequestDocument, String> {
        let w = self.workspace().ok_or("workspace_required")?;
        let identity = format!(
            "work-{}",
            crate::types::digest(format!("{}/{}", d.owner, self.work_id).as_bytes())
        );
        Ok(WorkUnitRequestDocument {
            version: "rack-ai/work-unit/v2".into(),
            workload: WorkloadDocument {
                id: "reserved-work".into(),
                kind: "application-development".into(),
            },
            repository: w.repository.clone(),
            work_unit: WorkUnitDocument {
                id: identity.clone(),
                objective: w.objective.clone(),
                allowed_paths: w.allowed_paths.clone(),
                acceptance: w.acceptance.clone(),
                environment_resources: w.environment_resources.clone(),
                readiness: Default::default(),
                requirements: w.requirements.clone(),
                limits: w.limits.clone(),
                routing: Some(GenericRoutingHeaderDocument {
                    source_system: d.owner.clone(),
                    work_id: self.work_id.clone(),
                    submission_id: identity.clone(),
                    idempotency_key: identity,
                    required_capabilities: d.request.capabilities.clone(),
                    priority: d.priority,
                }),
            },
        })
    }
}
pub fn is_workspace(i: &crate::types::Invocation) -> bool {
    i.request
        .work
        .as_ref()
        .is_some_and(|w| w.workspace().is_some())
}
