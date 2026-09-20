use rack_ai_application::GenericResourceAvailability;
use rack_ai_application::GenericResourceAvailabilityEvidence;
use rack_ai_application::GenericSelectionReason;
use rack_ai_application::GenericWorkerIneligibility;
use rack_ai_application::GenericWorkerIneligibilityReason;
use rack_ai_application::GenericWorkerSelectionDecision;
use rack_ai_application::WorkerCatalog;
use rack_ai_application::WorkspaceRequest;
use rack_ai_application::WorkspaceSelectionError;
use rack_ai_application::WorkspaceWorkerSelection;
use rack_ai_application::WorkspaceWorkerSelector;

use crate::FileSystemRegistryRepository;
use crate::FileSystemWorkerCatalog;
use crate::JCodeWorkerConfigResolver;
use crate::ModelRecord;
use crate::RegistryPaths;
use crate::ResourceRecord;
use crate::WorkerRecord;

pub struct RegistryWorkspaceWorkerSelector {
    repository: FileSystemRegistryRepository,
    catalog: FileSystemWorkerCatalog,
    resolver: JCodeWorkerConfigResolver,
    reserved_worker: Option<String>,
    reserved_context_window: Option<u32>,
}

impl RegistryWorkspaceWorkerSelector {
    pub fn new(paths: RegistryPaths) -> Self {
        Self {
            reserved_worker: None,
            repository: FileSystemRegistryRepository::new(paths.clone()),
            catalog: FileSystemWorkerCatalog::new(paths.clone()),
            resolver: JCodeWorkerConfigResolver::new(paths),
            reserved_context_window: None,
        }
    }
}

impl WorkspaceWorkerSelector for RegistryWorkspaceWorkerSelector {
    fn select(
        &self,
        request: &WorkspaceRequest,
    ) -> Result<WorkspaceWorkerSelection, WorkspaceSelectionError> {
        let models = self
            .repository
            .load_models()
            .map_err(WorkspaceSelectionError::Other)?;
        let mut workers = self
            .repository
            .load_workers()
            .map_err(WorkspaceSelectionError::Other)?;
        if let Some(id) = &self.reserved_worker {
            workers.retain(|w| &w.id == id);
        }
        let resources = self
            .repository
            .load_resources()
            .map_err(WorkspaceSelectionError::Other)?;
        select_generic(
            request,
            &SelectionContext {
                workers: &workers,
                models: &models,
                resources: &resources,
                resolver: &self.resolver,
                catalog: &self.catalog,
                reserved_context_window: self.reserved_context_window,
            },
        )
    }
}

struct SelectionContext<'a> {
    workers: &'a [WorkerRecord],
    models: &'a [ModelRecord],
    resources: &'a [ResourceRecord],
    resolver: &'a JCodeWorkerConfigResolver,
    catalog: &'a FileSystemWorkerCatalog,
    reserved_context_window: Option<u32>,
}
struct Candidates<'a> {
    decision: GenericWorkerSelectionDecision,
    eligible: Vec<(
        &'a WorkerRecord,
        &'a rack_ai_application::GenericModelEligibilityProfile,
    )>,
    temporarily_unavailable: bool,
}
fn candidates<'a>(request: &WorkspaceRequest, context: &SelectionContext<'a>) -> Candidates<'a> {
    let SelectionContext {
        workers,
        models,
        resources,
        ..
    } = context;
    let routing = request.routing();
    let mut decision = GenericWorkerSelectionDecision::new(
        routing,
        request.complexity(),
        request.requires_large_context(),
    );
    let mut eligible = Vec::new();
    let mut temporarily_unavailable = false;
    for worker in *workers {
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
    Candidates {
        decision,
        eligible,
        temporarily_unavailable,
    }
}
fn select_generic(
    request: &WorkspaceRequest,
    context: &SelectionContext<'_>,
) -> Result<WorkspaceWorkerSelection, WorkspaceSelectionError> {
    let Candidates {
        mut decision,
        eligible,
        temporarily_unavailable,
    } = candidates(request, context);
    let (worker, profile) = match eligible.first().copied() {
        Some(value) => value,
        None if temporarily_unavailable => {
            return Err(WorkspaceSelectionError::TemporarilyUnavailable);
        }
        None => return Err(WorkspaceSelectionError::CapabilityUnavailable),
    };
    decision.selected_worker_id = Some(worker.id.clone());
    decision.selection_reason = Some(if eligible.len() == 1 {
        GenericSelectionReason::OnlyEligible
    } else {
        GenericSelectionReason::LeastScarceSufficient
    });
    decision.model_profile_version = Some(profile.profile_version.clone());
    decision.qualification_evidence_refs = profile.qualification_evidence_refs.clone();
    let mut runtime = context
        .resolver
        .resolve(worker.id.as_str())
        .map_err(WorkspaceSelectionError::Other)?;
    if let Some(context_window) = context.reserved_context_window {
        runtime = runtime.with_context_window(Some(context_window));
    }
    let placement = context
        .catalog
        .resolve(worker.id.as_str())
        .map_err(WorkspaceSelectionError::Other)?
        .placement();
    Ok(WorkspaceWorkerSelection::new(runtime, placement).with_selection_decision(decision))
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
    request: &WorkspaceRequest,
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::WorkspaceRequest;
    use rack_ai_application::WorkspaceWorkerSelector;

    use super::RegistryWorkspaceWorkerSelector;
    use crate::RegistryPaths;

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
    fn reserved_primary_uses_reserved_profile_context_without_changing_identity() {
        let root = temp_root();
        write_generic_registry(&root, "active");
        let selector = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root))
            .for_reserved_worker("local-primary".to_string())
            .with_reserved_context_window(65_536);

        let selection = selector
            .select(&generic_request(
                vec!["reasoning", "coding"],
                "medium",
                false,
                "low",
                "athba",
            ))
            .unwrap();

        let runtime = selection.runtime();
        assert_eq!(runtime.worker_id(), "local-primary");
        assert_eq!(runtime.api_model_id(), "local-primary");
        assert_eq!(runtime.endpoint(), "http://127.0.0.1:8017/v1");
        assert_eq!(runtime.context_window(), Some(65_536));
        assert_eq!(
            runtime.worker_provenance().unwrap().resource_id,
            "gpu-4060ti"
        );
        assert_eq!(
            selection
                .selection_decision()
                .unwrap()
                .selected_worker_id
                .as_deref(),
            Some("local-primary")
        );
    }

    #[test]
    fn reserved_coder_uses_reserved_profile_context_without_changing_identity() {
        let root = temp_root();
        write_generic_registry(&root, "active");
        let selector = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root))
            .for_reserved_worker("local-coder".to_string())
            .with_reserved_context_window(14_320);

        let selection = selector
            .select(&generic_request(
                vec!["coding"],
                "small",
                false,
                "low",
                "athba",
            ))
            .unwrap();

        let runtime = selection.runtime();
        assert_eq!(runtime.worker_id(), "local-coder");
        assert_eq!(runtime.api_model_id(), "local-coder");
        assert_eq!(runtime.endpoint(), "http://127.0.0.1:8018/v1");
        assert_eq!(runtime.context_window(), Some(14_320));
        assert_eq!(runtime.tool_profile(), Some("minimal"));
        assert_eq!(runtime.worker_provenance().unwrap().resource_id, "gpu-2060");
    }

    #[test]
    fn reserved_context_window_changes_independently_of_registry_model_metadata() {
        let root = temp_root();
        write_generic_registry(&root, "active");
        let request = generic_request(vec!["coding"], "small", false, "low", "athba");

        let first = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root.clone()))
            .for_reserved_worker("local-coder".to_string())
            .with_reserved_context_window(14_320)
            .select(&request)
            .unwrap();
        let second = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root))
            .for_reserved_worker("local-coder".to_string())
            .with_reserved_context_window(12_000)
            .select(&request)
            .unwrap();

        assert_eq!(first.runtime().worker_id(), second.runtime().worker_id());
        assert_eq!(
            first.runtime().api_model_id(),
            second.runtime().api_model_id()
        );
        assert_eq!(first.runtime().context_window(), Some(14_320));
        assert_eq!(second.runtime().context_window(), Some(12_000));
    }

    #[test]
    fn generic_coding_small_selects_least_scarce_coder_and_persists_decision() {
        let root = temp_root();
        write_generic_registry(&root, "active");
        let selector = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root));
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
        let selector = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root));
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
    fn generic_admission_accepts_all_principals_at_global_priorities() {
        let root = temp_root();
        write_generic_registry(&root, "active");
        let selector = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root));
        for (source, priority) in [("athba", "high"), ("ATHBA", "paramount")] {
            assert!(
                selector
                    .select(&generic_request(
                        vec!["coding"],
                        "small",
                        false,
                        priority,
                        source
                    ))
                    .is_ok()
            );
        }
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
        let selector = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root.clone()));
        assert_eq!(
            selector.select(&generic_request(
                vec!["coding"],
                "small",
                false,
                "medium",
                "neutral"
            )),
            Err(rack_ai_application::WorkspaceSelectionError::TemporarilyUnavailable)
        );
        write_generic_registry(&root, "active");
        let selector = RegistryWorkspaceWorkerSelector::new(RegistryPaths::new(root));
        assert_eq!(
            selector.select(&generic_request(
                vec!["visual"],
                "small",
                false,
                "medium",
                "neutral"
            )),
            Err(rack_ai_application::WorkspaceSelectionError::CapabilityUnavailable)
        );
    }

    fn generic_request(
        capabilities: Vec<&str>,
        complexity: &str,
        large_context: bool,
        priority: &str,
        source: &str,
    ) -> WorkspaceRequest {
        rack_ai_application::WorkspaceRequest {
            change: serde_json::from_value(serde_json::json!({"change_id":"neutral-001",
                "repository":{"id":"neutral","base_ref":"main"}, "task":"Make one bounded change.",
                "allowed_paths":["src/"],"acceptance":{"commands":[["cargo","test"]]},
                "limits":{"max_implementation_attempts":1,"timeout_seconds":30}})).unwrap(),
            requirements: serde_json::from_value(serde_json::json!({"complexity":complexity,"requires_large_context":large_context})).unwrap(),
            routing: serde_json::from_value(serde_json::json!({"source_system":source,"work_id":"opaque-work",
                "submission_id":"opaque-submission","idempotency_key":"opaque-key","required_capabilities":capabilities,"priority":priority})).unwrap(),
        }
    }

    fn write_generic_registry(root: &PathBuf, resource_status: &str) {
        write_registry(root);
        fs::write(root.join("config/resources.json"), format!(r#"{{"resources":[{{"id":"gpu-4060ti","type":"gpu","label":"Primary","vram_gb":16,"device_hint":"generic","max_concurrent_tasks":1,"owner":"local-primary","status":"{resource_status}"}},{{"id":"gpu-2060","type":"gpu","label":"Coder","vram_gb":6,"device_hint":"generic","max_concurrent_tasks":1,"owner":"local-coder","status":"{resource_status}"}}]}}"#)).unwrap();
        fs::write(root.join("config/models.json"), r#"{"models":[
{"id":"gemma4-12b-local-primary","label":"Primary","role":"generic","backend":"vllm","worker_id":"local-primary","api_model_id":"local-primary","endpoint":"http://127.0.0.1:8017/v1","port":8017,"status":"active","eligibility_profile":{"model_profile_id":"local-primary-v1","capabilities":["reasoning","coding"],"max_complexity":"large","large_context_eligible":true,"qualification_status":"qualified","qualification_evidence_refs":["proof-primary"],"profile_version":"v1","execution_constraints":["configured-jcode-route"]}},
{"id":"eqaq-v2-local-coder","label":"Coder","role":"generic","backend":"vllm","worker_id":"local-coder","api_model_id":"local-coder","endpoint":"http://127.0.0.1:8018/v1","port":8018,"status":"active","context_window":16368,"eligibility_profile":{"model_profile_id":"local-coder-v1","capabilities":["coding"],"max_complexity":"small","large_context_eligible":false,"qualification_status":"qualified_with_constraints","qualification_evidence_refs":["proof-coder"],"profile_version":"v1","execution_constraints":["minimal-tool-profile"]}}
]}"#).unwrap();
    }
}

impl RegistryWorkspaceWorkerSelector {
    pub fn for_reserved_worker(mut self, id: String) -> Self {
        self.reserved_worker = Some(id);
        self
    }

    pub fn with_reserved_context_window(mut self, context_window: u32) -> Self {
        self.reserved_context_window = Some(context_window);
        self
    }
}
