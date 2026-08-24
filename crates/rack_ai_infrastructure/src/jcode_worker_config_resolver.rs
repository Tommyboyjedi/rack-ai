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
        let provider_profile = worker
            .provider_profile
            .clone()
            .ok_or_else(|| format!("worker missing provider_profile: {worker_id}"))?;
        let models = self.repository.load_models()?;
        let model = models
            .into_iter()
            .find(|item| item.worker_id == worker_id && item.status == "active")
            .ok_or_else(|| format!("no active model bound to worker: {worker_id}"))?;
        Ok(ImplementWorkerRuntime::new(
            worker.id,
            worker.entrypoint,
            provider_profile,
            model.api_model_id.unwrap_or_else(|| worker_id.to_string()),
            model.endpoint,
        )
        .with_tool_profile(worker.tool_profile))
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
    fn resolves_jcode_runtime_with_profile_model_and_endpoint() {
        let root = temp_root();
        write_registry(&root);
        let resolver = JCodeWorkerConfigResolver::new(RegistryPaths::new(root));

        let runtime = resolver.resolve("local-coder").unwrap();

        assert_eq!(runtime.worker_id(), "local-coder");
        assert_eq!(runtime.entrypoint(), "/home/tomp/.local/bin/jcode");
        assert_eq!(runtime.provider_profile(), "local-coder");
        assert_eq!(runtime.api_model_id(), "local-coder");
        assert_eq!(runtime.endpoint(), "http://127.0.0.1:8018/v1");
        assert_eq!(runtime.tool_profile(), Some("minimal"));
    }

    #[test]
    fn resolves_default_implementer_from_registry_role() {
        let root = temp_root();
        write_registry(&root);
        let resolver = JCodeWorkerConfigResolver::new(RegistryPaths::new(root));

        let runtime = resolver.resolve_default_implementer().unwrap();

        assert_eq!(runtime.worker_id(), "local-coder");
        assert_eq!(runtime.provider_profile(), "local-coder");
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
      "label": "cyankiwi/gemma-4-12B-it-AWQ-INT4",
      "role": "planner_verifier",
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
      "role": "implementer",
      "backend": "vllm",
      "worker_id": "local-coder",
      "api_model_id": "local-coder",
      "endpoint": "http://127.0.0.1:8018/v1",
      "port": 8018,
      "status": "active"
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
        let root = std::env::temp_dir().join(format!("rack-ai-jcode-config-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
