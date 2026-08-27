use std::collections::BTreeSet;

use rack_ai_domain::AllowedPath;
use rack_ai_domain::AllowedPaths;
use rack_ai_domain::ChangeTask;
use rack_ai_domain::WorkUnitCapability;
use rack_ai_domain::WorkUnitComplexity;
use rack_ai_domain::WorkUnitId;
use rack_ai_domain::WorkloadId;
use rack_ai_domain::WorkloadKind;

use crate::AcceptanceDocument;
use crate::ChangeRepositoryDocument;
use crate::ChangeRequestDocument;
use crate::LimitsDocument;
use crate::WorkUnitRequestDocument;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkUnitRequest {
    workload_id: WorkloadId,
    workload_kind: WorkloadKind,
    repository: ChangeRepositoryDocument,
    work_unit_id: WorkUnitId,
    objective: ChangeTask,
    allowed_paths: Vec<String>,
    acceptance: AcceptanceDocument,
    dependency_ids: Vec<WorkUnitId>,
    capability: WorkUnitCapability,
    complexity: WorkUnitComplexity,
    requires_large_context: bool,
    limits: LimitsDocument,
}

impl WorkUnitRequest {
    pub fn from_document(document: WorkUnitRequestDocument) -> Result<Self, String> {
        if document.version.trim() != "rack-ai/work-unit/v1" {
            return Err("unsupported work unit version".to_string());
        }
        let workload_id = WorkloadId::new(document.workload.id)?;
        let workload_kind = parse_workload_kind(&document.workload.kind)?;
        let work_unit_id = WorkUnitId::new(document.work_unit.id)?;
        let objective = ChangeTask::new(document.work_unit.objective)?;
        validate_allowed_paths(&document.work_unit.allowed_paths)?;
        if !document.work_unit.readiness.ready {
            return Err("work unit is not marked ready for execution".to_string());
        }
        let dependency_ids =
            validate_dependencies(&work_unit_id, &document.work_unit.readiness.depends_on)?;
        let capability = parse_capability(&document.work_unit.requirements.capability)?;
        let complexity = parse_complexity(&document.work_unit.requirements.complexity)?;
        Ok(Self {
            workload_id,
            workload_kind,
            repository: ChangeRepositoryDocument {
                id: document.repository.id,
                registered_root: document.repository.registered_root,
                base_ref: document.repository.base_ref,
                base_sha: document.repository.base_sha,
            },
            work_unit_id,
            objective,
            allowed_paths: document.work_unit.allowed_paths,
            acceptance: AcceptanceDocument {
                commands: document.work_unit.acceptance.commands,
                required_artifacts: document.work_unit.acceptance.required_artifacts,
            },
            dependency_ids,
            capability,
            complexity,
            requires_large_context: document.work_unit.requirements.requires_large_context,
            limits: LimitsDocument {
                max_implementation_attempts: document.work_unit.limits.max_implementation_attempts,
                timeout_seconds: document.work_unit.limits.timeout_seconds,
                network: document.work_unit.limits.network,
            },
        })
    }

    pub fn workload_id(&self) -> &WorkloadId {
        &self.workload_id
    }

    pub fn workload_kind(&self) -> WorkloadKind {
        self.workload_kind
    }

    pub fn work_unit_id(&self) -> &WorkUnitId {
        &self.work_unit_id
    }

    pub fn capability(&self) -> WorkUnitCapability {
        self.capability
    }

    pub fn complexity(&self) -> WorkUnitComplexity {
        self.complexity
    }

    pub fn requires_large_context(&self) -> bool {
        self.requires_large_context
    }

    pub fn dependency_ids(&self) -> &[WorkUnitId] {
        self.dependency_ids.as_slice()
    }

    pub fn change_id(&self) -> String {
        format!(
            "{}--{}",
            self.workload_id.value(),
            self.work_unit_id.value()
        )
    }

    pub fn to_change_request_document(&self) -> ChangeRequestDocument {
        ChangeRequestDocument {
            change_id: self.change_id(),
            repository: self.repository.clone(),
            task: self.objective.value().to_string(),
            allowed_paths: self.allowed_paths.clone(),
            acceptance: self.acceptance.clone(),
            limits: self.limits.clone(),
        }
    }
}

fn parse_workload_kind(value: &str) -> Result<WorkloadKind, String> {
    match value.trim() {
        "application-development" => Ok(WorkloadKind::ApplicationDevelopment),
        _ => Err("unsupported workload kind".to_string()),
    }
}

fn parse_capability(value: &str) -> Result<WorkUnitCapability, String> {
    match value.trim() {
        "implementation" => Ok(WorkUnitCapability::Implementation),
        _ => Err("unsupported work unit capability".to_string()),
    }
}

fn parse_complexity(value: &str) -> Result<WorkUnitComplexity, String> {
    match value.trim() {
        "small" => Ok(WorkUnitComplexity::Small),
        "medium" => Ok(WorkUnitComplexity::Medium),
        "large" => Ok(WorkUnitComplexity::Large),
        _ => Err("unsupported work unit complexity".to_string()),
    }
}

fn validate_allowed_paths(paths: &[String]) -> Result<(), String> {
    let parsed = paths
        .iter()
        .cloned()
        .map(AllowedPath::new)
        .collect::<Result<Vec<_>, _>>()?;
    AllowedPaths::new(parsed).map(|_| ())
}

fn validate_dependencies(
    work_unit_id: &WorkUnitId,
    values: &[String],
) -> Result<Vec<WorkUnitId>, String> {
    let mut seen = BTreeSet::new();
    let mut dependencies = Vec::new();
    for value in values {
        let dependency = WorkUnitId::new(value.clone())?;
        if dependency == *work_unit_id {
            return Err("work unit cannot depend on itself".to_string());
        }
        if !seen.insert(dependency.value().to_string()) {
            return Err("work unit dependencies must be unique".to_string());
        }
        dependencies.push(dependency);
    }
    Ok(dependencies)
}

#[cfg(test)]
mod tests {
    use super::WorkUnitRequest;
    use crate::WorkUnitRequestDocument;

    #[test]
    fn rejects_not_ready_work_unit() {
        let document = serde_json::from_value(serde_json::json!({
            "version": "rack-ai/work-unit/v1",
            "workload": {"id": "adaptos", "kind": "application-development"},
            "repository": {"id": "adaptos", "base_ref": "main"},
            "work_unit": {
                "id": "adaptos-001",
                "objective": "Implement a bounded feature.",
                "allowed_paths": ["src/"],
                "acceptance": {"commands": [["cargo", "test"]]},
                "readiness": {"ready": false},
                "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
            }
        }))
        .unwrap();
        assert_eq!(
            WorkUnitRequest::from_document(document),
            Err("work unit is not marked ready for execution".to_string())
        );
    }

    #[test]
    fn rejects_self_dependency() {
        let document = serde_json::from_value::<WorkUnitRequestDocument>(serde_json::json!({
            "version": "rack-ai/work-unit/v1",
            "workload": {"id": "adaptos", "kind": "application-development"},
            "repository": {"id": "adaptos", "base_ref": "main"},
            "work_unit": {
                "id": "adaptos-001",
                "objective": "Implement a bounded feature.",
                "allowed_paths": ["src/"],
                "acceptance": {"commands": [["cargo", "test"]]},
                "readiness": {"depends_on": ["adaptos-001"]},
                "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
            }
        }))
        .unwrap();
        assert_eq!(
            WorkUnitRequest::from_document(document),
            Err("work unit cannot depend on itself".to_string())
        );
    }
}
