use rack_ai_application::WorkUnitRequest;
use rack_ai_application::WorkUnitWorkerSelection;
use rack_ai_application::WorkUnitWorkerSelector;
use rack_ai_application::WorkerCatalog;
use rack_ai_domain::WorkUnitCapability;

use crate::FileSystemRegistryRepository;
use crate::FileSystemWorkerCatalog;
use crate::JCodeWorkerConfigResolver;
use crate::RegistryPaths;
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
    fn select(&self, request: &WorkUnitRequest) -> Result<WorkUnitWorkerSelection, String> {
        if request.capability() != WorkUnitCapability::Implementation {
            return Err("unsupported work unit capability".to_string());
        }
        let models = self.repository.load_models()?;
        let workers = self.repository.load_workers()?;
        let active_model_workers = models
            .iter()
            .filter(|item| item.status == "active")
            .map(|item| item.worker_id.as_str())
            .collect::<Vec<_>>();
        let worker = choose_worker(request, &workers, &active_model_workers)?;
        let runtime = self.resolver.resolve(worker.id.as_str())?;
        let placement = self.catalog.resolve(worker.id.as_str())?.placement();
        Ok(WorkUnitWorkerSelection::new(runtime, placement))
    }
}

fn choose_worker<'a>(
    request: &WorkUnitRequest,
    workers: &'a [WorkerRecord],
    active_model_workers: &[&str],
) -> Result<&'a WorkerRecord, String> {
    let candidates = workers
        .iter()
        .filter(|worker| worker.enabled)
        .filter(|worker| worker.kind == "jcode")
        .filter(|worker| active_model_workers.contains(&worker.id.as_str()))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err("no enabled JCode workers with active model bindings".to_string());
    }
    if request.requires_large_context() || request.complexity().prefers_stronger_worker() {
        return candidates
            .iter()
            .find(|worker| worker.tool_profile.as_deref() != Some("minimal"))
            .copied()
            .or_else(|| candidates.first().copied())
            .ok_or_else(|| "no worker available for stronger work unit".to_string());
    }
    candidates
        .iter()
        .find(|worker| {
            worker.tool_profile.as_deref() == Some("minimal") && worker.role.contains("implementer")
        })
        .copied()
        .or_else(|| {
            candidates
                .iter()
                .find(|worker| worker.role.contains("implementer"))
                .copied()
        })
        .or_else(|| candidates.first().copied())
        .ok_or_else(|| "no worker available for bounded implementation work".to_string())
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
}
