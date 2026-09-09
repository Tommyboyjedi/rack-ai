use rack_ai_application::ImplementWorkerRuntime;

use crate::FileSystemRegistryRepository;
use crate::RegistryPaths;

pub struct JCodeWorkerConfigResolver {
    repository: FileSystemRegistryRepository,
}

impl JCodeWorkerConfigResolver {
    pub fn new(paths: RegistryPaths) -> Self {
        Self {
            repository: FileSystemRegistryRepository::new(paths),
        }
    }

    pub fn resolve(&self, worker_id: &str) -> Result<ImplementWorkerRuntime, String> {
        let workers = self.repository.load_workers()?;
        let worker = workers
            .into_iter()
            .find(|item| item.id == worker_id)
            .ok_or_else(|| format!("unknown worker: {worker_id}"))?;
        if !worker.enabled {
            return Err(format!("worker disabled: {worker_id}"));
        }
        if worker.kind != "jcode" {
            return Err(format!("worker is not configured for JCode: {worker_id}"));
        }
        let provenance = worker.execution_provenance()?;
        let provider_profile = worker
            .provider_profile
            .clone()
            .ok_or_else(|| format!("worker missing provider_profile: {worker_id}"))?;
        let models = self.repository.load_models()?;
        let model = models
            .into_iter()
            .find(|item| item.worker_id == worker_id && item.status == "active")
            .ok_or_else(|| format!("no active model bound to worker: {worker_id}"))?;
        let api_model_id = model
            .api_model_id
            .ok_or_else(|| format!("worker missing api_model_id binding: {worker_id}"))?;
        if worker.tool_profile.as_deref() == Some("minimal") && model.context_window.is_none() {
            return Err(format!(
                "worker {} requires context_window for minimal JCode execution",
                worker_id
            ));
        }
        Ok(ImplementWorkerRuntime::new(
            worker.id,
            worker.entrypoint,
            provider_profile,
            api_model_id,
            model.endpoint,
        )
        .with_tool_profile(worker.tool_profile)
        .with_context_window(model.context_window)
        .with_worker_provenance(provenance))
    }

    pub fn resolve_default_implementer(&self) -> Result<ImplementWorkerRuntime, String> {
        let workers = self.repository.load_workers()?;
        let worker = workers
            .into_iter()
            .find(|item| item.enabled && item.kind == "jcode" && item.role.contains("implementer"))
            .ok_or_else(|| "no enabled JCode implementer worker configured".to_string())?;
        self.resolve(worker.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::JCodeWorkerConfigResolver;
    use crate::RegistryPaths;

    #[test]
    fn resolves_jcode_runtime_with_profile_model_endpoint_and_context() {
        let root = temp_root();
        write_registry(
            &root,
            Some("local-coder"),
            Some(16368),
            "http://127.0.0.1:8018/v1",
        );
        let resolver = JCodeWorkerConfigResolver::new(RegistryPaths::new(root));

        let runtime = resolver.resolve("local-coder").unwrap();

        assert_eq!(runtime.worker_id(), "local-coder");
        assert_eq!(runtime.entrypoint(), "/home/tomp/.local/bin/jcode");
        assert_eq!(runtime.provider_profile(), "local-coder");
        assert_eq!(runtime.api_model_id(), "local-coder");
        assert_eq!(runtime.endpoint(), "http://127.0.0.1:8018/v1");
        assert_eq!(runtime.tool_profile(), Some("minimal"));
        assert_eq!(runtime.context_window(), Some(16368));
        assert_eq!(
            runtime.worker_provenance().unwrap().model_id,
            "eqaq-v2-local-coder"
        );
    }

    #[test]
    fn resolves_default_implementer_from_registry_role() {
        let root = temp_root();
        write_registry(
            &root,
            Some("local-coder"),
            Some(16368),
            "http://127.0.0.1:8018/v1",
        );
        let resolver = JCodeWorkerConfigResolver::new(RegistryPaths::new(root));

        let runtime = resolver.resolve_default_implementer().unwrap();

        assert_eq!(runtime.worker_id(), "local-coder");
        assert_eq!(runtime.provider_profile(), "local-coder");
    }

    #[test]
    fn rejects_missing_api_model_binding() {
        let root = temp_root();
        write_registry(&root, None, Some(16368), "http://127.0.0.1:8018/v1");
        let resolver = JCodeWorkerConfigResolver::new(RegistryPaths::new(root));

        let error = resolver.resolve("local-coder").unwrap_err();

        assert!(error.contains("api_model_id"));
    }

    #[test]
    fn rejects_minimal_worker_without_context_window() {
        let root = temp_root();
        write_registry(&root, Some("local-coder"), None, "http://127.0.0.1:8018/v1");
        let resolver = JCodeWorkerConfigResolver::new(RegistryPaths::new(root));

        let error = resolver.resolve("local-coder").unwrap_err();

        assert!(error.contains("context_window"));
    }

    fn write_registry(
        root: &PathBuf,
        api_model_id: Option<&str>,
        context_window: Option<u32>,
        endpoint: &str,
    ) {
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
        let api_model_json = api_model_id
            .map(|value| format!("\n      \"api_model_id\": \"{value}\","))
            .unwrap_or_default();
        let context_json = context_window
            .map(|value| format!("\n      \"context_window\": {value},"))
            .unwrap_or_default();
        fs::write(
            root.join("config/models.json"),
            format!(
                r#"{{
  "models": [
    {{
      "id": "gemma4-12b-local-primary",
      "label": "cyankiwi/gemma-4-12B-it-AWQ-INT4",
      "role": "planner_verifier",
      "backend": "vllm",
      "worker_id": "local-primary",
      "api_model_id": "local-primary",
      "endpoint": "http://127.0.0.1:8017/v1",
      "port": 8017,
      "status": "active"
    }},
    {{
      "id": "eqaq-v2-local-coder",
      "label": "NotaMG/eqaq-v2",
      "role": "implementer",
      "backend": "vllm",
      "worker_id": "local-coder",{api_model_json}{context_json}
      "endpoint": "{endpoint}",
      "port": 8018,
      "status": "active"
    }}
  ]
}}"#,
                api_model_json = api_model_json,
                context_json = context_json,
                endpoint = endpoint,
            ),
        )
        .unwrap();
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-jcode-config-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
