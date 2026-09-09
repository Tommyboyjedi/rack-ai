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
use crate::GenericRoutingHeader;
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
    environment_resources: Vec<String>,
    dependency_ids: Vec<WorkUnitId>,
    capability: WorkUnitCapability,
    complexity: WorkUnitComplexity,
    requires_large_context: bool,
    limits: LimitsDocument,
    routing: Option<GenericRoutingHeader>,
}

impl WorkUnitRequest {
    pub fn from_document(document: WorkUnitRequestDocument) -> Result<Self, String> {
        let routing = match document.version.trim() {
            "rack-ai/work-unit/v1" => None,
            "rack-ai/work-unit/v2" => {
                let value = document
                    .work_unit
                    .routing
                    .as_ref()
                    .ok_or("v2 work unit requires routing header")?;
                Some(GenericRoutingHeader::new(
                    value.source_system.clone(),
                    value.work_id.clone(),
                    value.submission_id.clone(),
                    value.idempotency_key.clone(),
                    value.required_capabilities.clone(),
                    value.priority,
                )?)
            }
            _ => return Err("unsupported work unit version".to_string()),
        };
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
                root: document.repository.root,
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
            environment_resources: document.work_unit.environment_resources,
            dependency_ids,
            capability,
            complexity,
            requires_large_context: document.work_unit.requirements.requires_large_context,
            limits: LimitsDocument {
                max_implementation_attempts: document.work_unit.limits.max_implementation_attempts,
                timeout_seconds: document.work_unit.limits.timeout_seconds,
                network: document.work_unit.limits.network,
            },
            routing,
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

    pub fn routing(&self) -> Option<&GenericRoutingHeader> {
        self.routing.as_ref()
    }

    pub fn is_generic_capability_routed(&self) -> bool {
        self.routing.is_some()
    }

    pub fn dependency_ids(&self) -> &[WorkUnitId] {
        self.dependency_ids.as_slice()
    }

    pub fn change_id(&self) -> String {
        let base = format!(
            "{}--{}",
            self.workload_id.value(),
            self.work_unit_id.value()
        );
        self.routing.as_ref().map_or(base.clone(), |routing| {
            format!(
                "{base}--submission-{}",
                opaque_suffix(&routing.submission_id)
            )
        })
    }

    pub fn to_change_request_document(&self) -> ChangeRequestDocument {
        ChangeRequestDocument {
            change_id: self.change_id(),
            repository: self.repository.clone(),
            task: self.objective.value().to_string(),
            allowed_paths: self.allowed_paths.clone(),
            acceptance: self.acceptance.clone(),
            environment_resources: self.environment_resources.clone(),
            limits: self.limits.clone(),
        }
    }
}

fn opaque_suffix(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .fold(14695981039346656037_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(1099511628211)
        })
        .to_string()
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

    #[test]
    fn parses_v2_capability_sets_and_preserves_opaque_identity() {
        for capabilities in [
            vec!["coding"],
            vec!["reasoning"],
            vec!["reasoning", "coding"],
        ] {
            let request =
                WorkUnitRequest::from_document(v2_document(capabilities.clone())).unwrap();
            let routing = request.routing().unwrap();
            assert_eq!(routing.work_id, "work-opaque");
            assert_eq!(routing.submission_id, "submission-opaque");
            assert_eq!(routing.required_capabilities.len(), capabilities.len());
        }
    }

    #[test]
    fn rejects_empty_duplicate_and_unknown_v2_capabilities() {
        assert!(WorkUnitRequest::from_document(v2_document(vec![])).is_err());
        assert!(WorkUnitRequest::from_document(v2_document(vec!["coding", "coding"])).is_err());
        let unknown = serde_json::from_value::<WorkUnitRequestDocument>(serde_json::json!({
            "version":"rack-ai/work-unit/v2", "workload":{"id":"w","kind":"application-development"}, "repository":{"id":"r","base_ref":"main"},
            "work_unit":{"id":"u","objective":"bounded", "allowed_paths":["src/"], "acceptance":{"commands":[["cargo","test"]]}, "limits":{"max_implementation_attempts":1,"timeout_seconds":1},
            "routing":{"source_system":"neutral","work_id":"work","submission_id":"submission","idempotency_key":"key","required_capabilities":["unknown"],"priority":"medium"}}}));
        assert!(unknown.is_err());
    }

    #[test]
    fn distinct_v2_submissions_for_one_work_have_distinct_transaction_ids() {
        let first = v2_document(vec!["coding"]);
        let mut second = v2_document(vec!["coding"]);
        second.work_unit.routing.as_mut().unwrap().submission_id = "submission-other".to_string();
        let first = WorkUnitRequest::from_document(first).unwrap();
        let second = WorkUnitRequest::from_document(second).unwrap();
        assert_ne!(first.change_id(), second.change_id());
    }

    fn v2_document(capabilities: Vec<&str>) -> WorkUnitRequestDocument {
        serde_json::from_value(serde_json::json!({
            "version":"rack-ai/work-unit/v2", "workload":{"id":"w","kind":"application-development"}, "repository":{"id":"r","base_ref":"main"},
            "work_unit":{"id":"u","objective":"bounded", "allowed_paths":["src/"], "acceptance":{"commands":[["cargo","test"]]}, "limits":{"max_implementation_attempts":1,"timeout_seconds":1},
            "routing":{"source_system":"neutral","work_id":"work-opaque","submission_id":"submission-opaque","idempotency_key":"key-opaque","required_capabilities":capabilities,"priority":"medium"}}
        })).unwrap()
    }
}
