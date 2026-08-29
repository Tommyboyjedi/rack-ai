use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkUnitRequestDocument {
    pub version: String,
    pub workload: WorkloadDocument,
    pub repository: WorkUnitRepositoryDocument,
    pub work_unit: WorkUnitDocument,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadDocument {
    pub id: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkUnitRepositoryDocument {
    pub id: String,
    #[serde(default)]
    pub registered_root: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    pub base_ref: String,
    #[serde(default)]
    pub base_sha: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkUnitDocument {
    pub id: String,
    pub objective: String,
    pub allowed_paths: Vec<String>,
    pub acceptance: WorkUnitAcceptanceDocument,
    #[serde(default)]
    pub readiness: WorkUnitReadinessDocument,
    #[serde(default)]
    pub requirements: WorkUnitRequirementsDocument,
    pub limits: WorkUnitLimitsDocument,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkUnitAcceptanceDocument {
    pub commands: Vec<Vec<String>>,
    #[serde(default)]
    pub required_artifacts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkUnitReadinessDocument {
    #[serde(default = "ready_default")]
    pub ready: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

impl Default for WorkUnitReadinessDocument {
    fn default() -> Self {
        Self {
            ready: true,
            depends_on: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkUnitRequirementsDocument {
    #[serde(default = "capability_default")]
    pub capability: String,
    #[serde(default = "complexity_default")]
    pub complexity: String,
    #[serde(default)]
    pub requires_large_context: bool,
}

impl Default for WorkUnitRequirementsDocument {
    fn default() -> Self {
        Self {
            capability: capability_default(),
            complexity: complexity_default(),
            requires_large_context: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkUnitLimitsDocument {
    pub max_implementation_attempts: u32,
    pub timeout_seconds: u32,
    #[serde(default = "network_default")]
    pub network: String,
}

fn ready_default() -> bool {
    true
}

fn capability_default() -> String {
    "implementation".to_string()
}

fn complexity_default() -> String {
    "small".to_string()
}

fn network_default() -> String {
    "disabled".to_string()
}

#[cfg(test)]
mod tests {
    use super::WorkUnitRequestDocument;

    #[test]
    fn parses_minimum_work_unit_document() {
        let json = r#"{
          "version": "rack-ai/work-unit/v1",
          "workload": {"id": "adaptos", "kind": "application-development"},
          "repository": {"id": "adaptos", "base_ref": "main"},
          "work_unit": {
            "id": "adaptos-001",
            "objective": "Implement a bounded feature.",
            "allowed_paths": ["src/"],
            "acceptance": {"commands": [["cargo", "test", "save_single_open_ticket"]]},
            "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
          }
        }"#;
        let document = serde_json::from_str::<WorkUnitRequestDocument>(json).unwrap();
        assert_eq!(document.work_unit.requirements.capability, "implementation");
        assert_eq!(document.work_unit.requirements.complexity, "small");
        assert!(document.work_unit.readiness.ready);
        assert_eq!(document.work_unit.limits.network, "disabled");
        assert!(document.repository.root.is_none());
    }

    #[test]
    fn rejects_unknown_worker_field() {
        let json = r#"{
          "version": "rack-ai/work-unit/v1",
          "workload": {"id": "adaptos", "kind": "application-development"},
          "repository": {"id": "adaptos", "base_ref": "main"},
          "work_unit": {
            "id": "adaptos-001",
            "objective": "Implement a bounded feature.",
            "allowed_paths": ["src/"],
            "acceptance": {"commands": [["cargo", "test"]]},
            "requirements": {"worker_id": "local-coder"},
            "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
          }
        }"#;
        assert!(serde_json::from_str::<WorkUnitRequestDocument>(json).is_err());
    }
}
