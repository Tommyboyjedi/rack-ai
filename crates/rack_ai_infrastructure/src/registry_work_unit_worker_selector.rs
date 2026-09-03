use rack_ai_application::GenericResourceAvailability;
use rack_ai_application::GenericResourceAvailabilityEvidence;
use rack_ai_application::GenericSelectionReason;
use rack_ai_application::GenericSourceAdmissionPolicy;
use rack_ai_application::GenericWorkerIneligibility;
use rack_ai_application::GenericWorkerIneligibilityReason;
use rack_ai_application::GenericWorkerSelectionDecision;
use rack_ai_application::WorkUnitRequest;
use rack_ai_application::WorkUnitSelectionError;
use rack_ai_application::WorkUnitWorkerSelection;
use rack_ai_application::WorkUnitWorkerSelector;
use rack_ai_application::WorkerCatalog;
use rack_ai_domain::WorkUnitCapability;

use crate::FileSystemRegistryRepository;
use crate::FileSystemWorkerCatalog;
use crate::JCodeWorkerConfigResolver;
use crate::ModelRecord;
use crate::RegistryPaths;
use crate::ResourceRecord;
use crate::WorkerRecord;

pub struct RegistryWorkUnitWorkerSelector {
    repository: FileSystemRegistryRepository,
    catalog: FileSystemWorkerCatalog,
    resolver: JCodeWorkerConfigResolver,
}

impl RegistryWorkUnitWorkerSelector {
    pub fn new(paths: RegistryPaths) -> Self {
        Self {
            repository: FileSystemRegistryRepository::new(paths.clone()),
            catalog: FileSystemWorkerCatalog::new(paths.clone()),
            resolver: JCodeWorkerConfigResolver::new(paths),
        }
    }
}

impl WorkUnitWorkerSelector for RegistryWorkUnitWorkerSelector {
    fn select(
        &self,
        request: &WorkUnitRequest,
    ) -> Result<WorkUnitWorkerSelection, WorkUnitSelectionError> {
        let models = self
            .repository
            .load_models()
            .map_err(WorkUnitSelectionError::Other)?;
        let workers = self
            .repository
            .load_workers()
            .map_err(WorkUnitSelectionError::Other)?;
        if let Some(routing) = request.routing() {
            let resources = self
                .repository
                .load_resources()
                .map_err(WorkUnitSelectionError::Other)?;
            let policies = self
                .repository
                .load_source_admission_policies()
                .map_err(WorkUnitSelectionError::Other)?;
            return select_generic(
                request,
                routing,
                &policies,
                &workers,
                &models,
                &resources,
                &self.resolver,
                &self.catalog,
            );
        }
        if request.capability() != WorkUnitCapability::Implementation {
            return Err(WorkUnitSelectionError::Other(
                "unsupported work unit capability".to_string(),
            ));
        }
        let active_model_workers = models
            .iter()
            .filter(|item| item.status == "active")
            .map(|item| item.worker_id.as_str())
            .collect::<Vec<_>>();
        let worker = choose_worker(request, &workers, &active_model_workers)?;
        let runtime = self
            .resolver
            .resolve(worker.id.as_str())
            .map_err(WorkUnitSelectionError::Other)?;
        let placement = self
            .catalog
            .resolve(worker.id.as_str())
            .map_err(WorkUnitSelectionError::Other)?
            .placement();
        Ok(WorkUnitWorkerSelection::new(runtime, placement))
    }
}

fn select_generic(
    request: &WorkUnitRequest,
    routing: &rack_ai_application::GenericRoutingHeader,
    policies: &[GenericSourceAdmissionPolicy],
    workers: &[WorkerRecord],
    models: &[ModelRecord],
    resources: &[ResourceRecord],
    resolver: &JCodeWorkerConfigResolver,
    catalog: &FileSystemWorkerCatalog,
) -> Result<WorkUnitWorkerSelection, WorkUnitSelectionError> {
    let policy = policies
        .iter()
        .find(|item| item.source_system != "*" && item.matches(&routing.source_system))
        .or_else(|| policies.iter().find(|item| item.source_system == "*"));
    let policy = policy.ok_or(WorkUnitSelectionError::SourceAdmissionPolicyMissing)?;
    if !policy.admits(routing.priority) {
        return Err(WorkUnitSelectionError::SourceAdmissionDenied);
    }

    let mut decision = GenericWorkerSelectionDecision::new(
        routing,
        request.complexity(),
        request.requires_large_context(),
    );
    let mut eligible = Vec::new();
    let mut temporarily_unavailable = false;
    for worker in workers {
        let profile = match worker_profile(worker, models) {
            Ok(profile) => profile,
            Err(reason) => {
                add_ineligible(&mut decision, worker, reason);
                continue;
            }
        };
        if let Some(reason) = capability_reason(request, routing, worker, profile) {
            add_ineligible(&mut decision, worker, reason);
            continue;
        }
        let resource = match resources
            .iter()
            .find(|resource| resource.id == worker.resource_id)
        {
            Some(resource) => resource,
            None => {
                add_ineligible(
                    &mut decision,
                    worker,
                    GenericWorkerIneligibilityReason::ResourceBindingMissing,
                );
                continue;
            }
        };
        let available = model_is_active(worker, models) && resource.status == "active";
        decision
            .resource_availability_evidence
            .push(GenericResourceAvailabilityEvidence {
                worker_id: worker.id.clone(),
                resource_id: worker.resource_id.clone(),
                availability: if available {
                    GenericResourceAvailability::Available
                } else {
                    GenericResourceAvailability::TemporarilyUnavailable
                },
            });
        if available {
            eligible.push((worker, profile));
        } else {
            temporarily_unavailable = true;
            add_ineligible(
                &mut decision,
                worker,
                GenericWorkerIneligibilityReason::TemporarilyUnavailable,
            );
        }
    }
    eligible.sort_by(
        |(left_worker, left_profile), (right_worker, right_profile)| {
            left_profile
                .capabilities
                .len()
                .cmp(&right_profile.capabilities.len())
                .then_with(|| left_worker.id.cmp(&right_worker.id))
        },
    );
    decision.eligible_worker_ids = eligible
        .iter()
        .map(|(worker, _)| worker.id.clone())
        .collect();
    let (worker, profile) = match eligible.first().copied() {
        Some(value) => value,
        None if temporarily_unavailable => {
            return Err(WorkUnitSelectionError::TemporarilyUnavailable);
        }
        None => return Err(WorkUnitSelectionError::CapabilityUnavailable),
    };
    decision.selected_worker_id = Some(worker.id.clone());
    decision.selection_reason = Some(if eligible.len() == 1 {
        GenericSelectionReason::OnlyEligible
    } else {
        GenericSelectionReason::LeastScarceSufficient
    });
    decision.model_profile_version = Some(profile.profile_version.clone());
    decision.qualification_evidence_refs = profile.qualification_evidence_refs.clone();
    let runtime = resolver
        .resolve(worker.id.as_str())
        .map_err(WorkUnitSelectionError::Other)?;
    let placement = catalog
        .resolve(worker.id.as_str())
        .map_err(WorkUnitSelectionError::Other)?
        .placement();
    Ok(WorkUnitWorkerSelection::new(runtime, placement).with_selection_decision(decision))
}

fn worker_profile<'a>(
    worker: &WorkerRecord,
    models: &'a [ModelRecord],
) -> Result<&'a rack_ai_application::GenericModelEligibilityProfile, GenericWorkerIneligibilityReason>
{
    if !worker.enabled {
        return Err(GenericWorkerIneligibilityReason::WorkerDisabled);
    }
    if worker.kind != "jcode" {
        return Err(GenericWorkerIneligibilityReason::UnsupportedHarness);
    }
    let model = models
        .iter()
        .find(|model| model.id == worker.model_id)
        .ok_or(GenericWorkerIneligibilityReason::ModelBindingMissing)?;
    model
        .eligibility_profile
        .as_ref()
        .ok_or(GenericWorkerIneligibilityReason::EligibilityProfileMissing)
}

fn capability_reason(
    request: &WorkUnitRequest,
    routing: &rack_ai_application::GenericRoutingHeader,
    worker: &WorkerRecord,
    profile: &rack_ai_application::GenericModelEligibilityProfile,
) -> Option<GenericWorkerIneligibilityReason> {
    if !routing
        .required_capabilities
        .iter()
        .all(|capability| profile.capabilities.contains(capability))
    {
        return Some(GenericWorkerIneligibilityReason::CapabilityUnsupported);
    }
    if !complexity_permits(profile.max_complexity, request.complexity()) {
        return Some(GenericWorkerIneligibilityReason::ComplexityUnqualified);
    }
    if request.requires_large_context() && !profile.large_context_eligible {
        return Some(GenericWorkerIneligibilityReason::LargeContextUnsupported);
    }
    if worker.id.is_empty() {
        return Some(GenericWorkerIneligibilityReason::ModelBindingMissing);
    }
    None
}

fn model_is_active(worker: &WorkerRecord, models: &[ModelRecord]) -> bool {
    models
        .iter()
        .find(|model| model.id == worker.model_id)
        .is_some_and(|model| model.status == "active")
}

fn add_ineligible(
    decision: &mut GenericWorkerSelectionDecision,
    worker: &WorkerRecord,
    reason: GenericWorkerIneligibilityReason,
) {
    decision
        .ineligible_workers_with_generic_reasons
        .push(GenericWorkerIneligibility {
            worker_id: worker.id.clone(),
            reason,
        });
}

fn complexity_permits(
    maximum: rack_ai_domain::WorkUnitComplexity,
    requested: rack_ai_domain::WorkUnitComplexity,
) -> bool {
    match (maximum, requested) {
        (rack_ai_domain::WorkUnitComplexity::Large, _)
        | (
            rack_ai_domain::WorkUnitComplexity::Medium,
            rack_ai_domain::WorkUnitComplexity::Small | rack_ai_domain::WorkUnitComplexity::Medium,
        )
        | (rack_ai_domain::WorkUnitComplexity::Small, rack_ai_domain::WorkUnitComplexity::Small) => {
            true
        }
        _ => false,
    }
}

fn choose_worker<'a>(
    request: &WorkUnitRequest,
    workers: &'a [WorkerRecord],
    active_model_workers: &[&str],
) -> Result<&'a WorkerRecord, WorkUnitSelectionError> {
    let candidates = workers
        .iter()
        .filter(|worker| worker.enabled)
        .filter(|worker| worker.kind == "jcode")
        .filter(|worker| active_model_workers.contains(&worker.id.as_str()))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(WorkUnitSelectionError::Other(
            "no enabled JCode workers with active model bindings".to_string(),
        ));
    }
    if request.requires_large_context() || request.complexity().prefers_stronger_worker() {
        return candidates
            .iter()
            .find(|worker| worker.tool_profile.as_deref() != Some("minimal"))
            .copied()
            .or_else(|| candidates.first().copied())
            .ok_or_else(|| {
                WorkUnitSelectionError::Other(
                    "no worker available for stronger work unit".to_string(),
                )
            });
    }
    candidates
        .iter()
        .find(|worker| worker.tool_profile.as_deref() == Some("minimal"))
        .copied()
        .or_else(|| {
            candidates
                .iter()
                .find(|worker| worker.tool_profile.as_deref() == Some("minimal"))
                .copied()
        })
        .or_else(|| candidates.first().copied())
        .ok_or_else(|| {
            WorkUnitSelectionError::Other(
                "no worker available for bounded implementation work".to_string(),
            )
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::WorkUnitRequest;
    use rack_ai_application::WorkUnitRequestDocument;
    use rack_ai_application::WorkUnitWorkerSelector;

    use super::RegistryWorkUnitWorkerSelector;
    use crate::RegistryPaths;

    #[test]
    fn selects_minimal_implementer_for_small_work() {
        let root = temp_root();
        write_registry(&root);
        let selector = RegistryWorkUnitWorkerSelector::new(RegistryPaths::new(root));
        let selection = selector.select(&sample_request(false, "small")).unwrap();
        assert_eq!(selection.runtime().worker_id(), "local-coder");
        assert_eq!(
            selection.placement().resource_ids(),
            ["gpu-2060".to_string()]
        );
        assert_eq!(
            selection.runtime().worker_provenance().unwrap().worker_role,
            "implementer-tester"
        );
    }

    #[test]
    fn selects_stronger_worker_for_large_context_work() {
        let root = temp_root();
        write_registry(&root);
        let selector = RegistryWorkUnitWorkerSelector::new(RegistryPaths::new(root));
        let selection = selector.select(&sample_request(true, "medium")).unwrap();
        assert_eq!(selection.runtime().worker_id(), "local-primary");
        assert_eq!(
            selection.placement().resource_ids(),
            ["gpu-4060ti".to_string()]
        );
        assert_eq!(
            selection.runtime().worker_provenance().unwrap().worker_role,
            "planner-verifier"
        );
    }

    fn sample_request(requires_large_context: bool, complexity: &str) -> WorkUnitRequest {
        WorkUnitRequest::from_document(
            serde_json::from_value::<WorkUnitRequestDocument>(serde_json::json!({
                "version": "rack-ai/work-unit/v1",
                "workload": {"id": "adaptos", "kind": "application-development"},
                "repository": {"id": "adaptos", "base_ref": "main"},
                "work_unit": {
                    "id": "adaptos-001",
                    "objective": "Implement a bounded feature.",
                    "allowed_paths": ["src/"],
                    "acceptance": {"commands": [["cargo", "test"]]},
                    "requirements": {
                        "complexity": complexity,
                        "requires_large_context": requires_large_context
                    },
                    "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
                }
            }))
            .unwrap(),
        )
        .unwrap()
    }

    fn write_registry(root: &PathBuf) {
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/workers.json"),
            r#"{
  "workers": [
    {
      "id": "local-primary",
      "kind": "jcode",
      "role": "planner-verifier",
      "entrypoint": "/home/tomp/.local/bin/jcode",
      "backend": "jcode",
      "resource_id": "gpu-4060ti",
      "model_id": "gemma4-12b-local-primary",
      "enabled": true,
      "provider_profile": "local-primary"
    },
    {
      "id": "local-coder",
      "kind": "jcode",
      "role": "implementer-tester",
      "entrypoint": "/home/tomp/.local/bin/jcode",
      "backend": "jcode",
      "resource_id": "gpu-2060",
      "model_id": "eqaq-v2-local-coder",
      "enabled": true,
      "provider_profile": "local-coder",
      "tool_profile": "minimal"
    }
  ]
}"#,
        )
        .unwrap();
        fs::write(
            root.join("config/models.json"),
            r#"{
  "models": [
    {
      "id": "gemma4-12b-local-primary",
      "label": "Gemma 4 12B",
      "role": "planner-verifier",
      "backend": "vllm",
      "worker_id": "local-primary",
      "api_model_id": "local-primary",
      "endpoint": "http://127.0.0.1:8017/v1",
      "port": 8017,
      "status": "active"
    },
    {
      "id": "eqaq-v2-local-coder",
      "label": "NotaMG/eqaq-v2",
      "role": "implementer-tester",
      "backend": "vllm",
      "worker_id": "local-coder",
      "api_model_id": "local-coder",
      "endpoint": "http://127.0.0.1:8018/v1",
      "port": 8018,
      "status": "active",
      "context_window": 16368
    }
  ]
}"#,
        )
        .unwrap();
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-work-unit-selector-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn generic_coding_small_selects_least_scarce_coder_and_persists_decision() {
        let root = temp_root();
        write_generic_registry(&root, "active");
        let selector = RegistryWorkUnitWorkerSelector::new(RegistryPaths::new(root));
        let selection = selector
            .select(&generic_request(
                vec!["coding"],
                "small",
                false,
                "medium",
                "neutral",
            ))
            .unwrap();
        assert_eq!(selection.runtime().worker_id(), "local-coder");
        let decision = selection.selection_decision().unwrap();
        assert_eq!(decision.selected_worker_id.as_deref(), Some("local-coder"));
        assert_eq!(
            serde_json::to_value(decision).unwrap()["selection_reason"],
            "least_scarce_sufficient"
        );
        assert_eq!(
            decision.eligible_worker_ids,
            vec!["local-coder", "local-primary"]
        );
    }

    #[test]
    fn generic_reasoning_coding_medium_selects_primary_and_records_generic_exclusion() {
        let root = temp_root();
        write_generic_registry(&root, "active");
        let selector = RegistryWorkUnitWorkerSelector::new(RegistryPaths::new(root));
        let selection = selector
            .select(&generic_request(
                vec!["coding", "reasoning"],
                "medium",
                false,
                "medium",
                "neutral",
            ))
            .unwrap();
        assert_eq!(selection.runtime().worker_id(), "local-primary");
        let decision = selection.selection_decision().unwrap();
        assert_eq!(decision.requested_capabilities.len(), 2);
        assert_eq!(
            serde_json::to_value(decision).unwrap()["ineligible_workers_with_generic_reasons"][0]["reason"],
            "capability_unsupported"
        );
        assert!(
            !serde_json::to_string(decision)
                .unwrap()
                .to_ascii_lowercase()
                .contains("frontier")
        );
    }

    #[test]
    fn generic_admission_rejects_athba_above_medium_and_accepts_global_priorities() {
        let root = temp_root();
        write_generic_registry(&root, "active");
        let selector = RegistryWorkUnitWorkerSelector::new(RegistryPaths::new(root));
        assert_eq!(
            selector.select(&generic_request(
                vec!["coding"],
                "small",
                false,
                "high",
                "athba"
            )),
            Err(rack_ai_application::WorkUnitSelectionError::SourceAdmissionDenied)
        );
        assert_eq!(
            selector.select(&generic_request(
                vec!["coding"],
                "small",
                false,
                "paramount",
                "ATHBA"
            )),
            Err(rack_ai_application::WorkUnitSelectionError::SourceAdmissionDenied)
        );
        assert!(
            selector
                .select(&generic_request(
                    vec!["coding"],
                    "small",
                    false,
                    "paramount",
                    "neutral"
                ))
                .is_ok()
        );
    }

    #[test]
    fn generic_distinguishes_temporary_capacity_from_no_capability() {
        let root = temp_root();
        write_generic_registry(&root, "busy");
        let selector = RegistryWorkUnitWorkerSelector::new(RegistryPaths::new(root.clone()));
        assert_eq!(
            selector.select(&generic_request(
                vec!["coding"],
                "small",
                false,
                "medium",
                "neutral"
            )),
            Err(rack_ai_application::WorkUnitSelectionError::TemporarilyUnavailable)
        );
        write_generic_registry(&root, "active");
        let selector = RegistryWorkUnitWorkerSelector::new(RegistryPaths::new(root));
        assert_eq!(
            selector.select(&generic_request(
                vec!["visual"],
                "small",
                false,
                "medium",
                "neutral"
            )),
            Err(rack_ai_application::WorkUnitSelectionError::CapabilityUnavailable)
        );
    }

    fn generic_request(
        capabilities: Vec<&str>,
        complexity: &str,
        large_context: bool,
        priority: &str,
        source: &str,
    ) -> WorkUnitRequest {
        WorkUnitRequest::from_document(serde_json::from_value(serde_json::json!({
            "version": "rack-ai/work-unit/v2",
            "workload": {"id": "neutral", "kind": "application-development"},
            "repository": {"id": "neutral", "base_ref": "main"},
            "work_unit": {
                "id": "neutral-001", "objective": "Make one bounded change.", "allowed_paths": ["src/"],
                "acceptance": {"commands": [["cargo", "test"]]},
                "requirements": {"complexity": complexity, "requires_large_context": large_context},
                "limits": {"max_implementation_attempts": 1, "timeout_seconds": 30},
                "routing": {"source_system": source, "work_id": "opaque-work", "submission_id": "opaque-submission", "idempotency_key": "opaque-key", "required_capabilities": capabilities, "priority": priority}
            }
        })).unwrap()).unwrap()
    }

    fn write_generic_registry(root: &PathBuf, resource_status: &str) {
        write_registry(root);
        fs::write(root.join("config/resources.json"), format!(r#"{{"resources":[{{"id":"gpu-4060ti","type":"gpu","label":"Primary","vram_gb":16,"device_hint":"generic","max_concurrent_tasks":1,"owner":"local-primary","status":"{resource_status}"}},{{"id":"gpu-2060","type":"gpu","label":"Coder","vram_gb":6,"device_hint":"generic","max_concurrent_tasks":1,"owner":"local-coder","status":"{resource_status}"}}]}}"#)).unwrap();
        fs::write(root.join("config/models.json"), r#"{"source_admission_policies":[{"source_system":"athba","max_priority":"medium"},{"source_system":"*","max_priority":"paramount"}],"models":[
{"id":"gemma4-12b-local-primary","label":"Primary","role":"generic","backend":"vllm","worker_id":"local-primary","api_model_id":"local-primary","endpoint":"http://127.0.0.1:8017/v1","port":8017,"status":"active","eligibility_profile":{"model_profile_id":"local-primary-v1","capabilities":["reasoning","coding"],"max_complexity":"large","large_context_eligible":true,"qualification_status":"qualified","qualification_evidence_refs":["proof-primary"],"profile_version":"v1","execution_constraints":["configured-jcode-route"]}},
{"id":"eqaq-v2-local-coder","label":"Coder","role":"generic","backend":"vllm","worker_id":"local-coder","api_model_id":"local-coder","endpoint":"http://127.0.0.1:8018/v1","port":8018,"status":"active","context_window":16368,"eligibility_profile":{"model_profile_id":"local-coder-v1","capabilities":["coding"],"max_complexity":"small","large_context_eligible":false,"qualification_status":"qualified_with_constraints","qualification_evidence_refs":["proof-coder"],"profile_version":"v1","execution_constraints":["minimal-tool-profile"]}}
]}"#).unwrap();
    }
}
