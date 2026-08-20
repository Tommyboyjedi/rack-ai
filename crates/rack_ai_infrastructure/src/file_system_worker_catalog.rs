use rack_ai_application::WorkerBinding;
use rack_ai_application::WorkerCatalog;

use crate::FileSystemRegistryRepository;
use crate::RegistryPaths;

pub struct FileSystemWorkerCatalog {
    repository: FileSystemRegistryRepository,
}

impl FileSystemWorkerCatalog {
    pub fn new(paths: RegistryPaths) -> Self {
        Self {
            repository: FileSystemRegistryRepository::new(paths),
        }
    }
}

impl WorkerCatalog for FileSystemWorkerCatalog {
    fn resolve(&self, worker_id: &str) -> Result<WorkerBinding, String> {
        let worker = self
            .repository
            .load_workers()?
            .into_iter()
            .find(|item| item.id == worker_id)
            .ok_or(format!("unknown worker: {worker_id}"))?;
        if !worker.enabled {
            return Err(format!("worker disabled: {worker_id}"));
        }
        Ok(WorkerBinding::new(
            worker.id,
            worker.resource_id,
            worker.model_id,
            worker.backend,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::WorkerCatalog;

    use super::FileSystemWorkerCatalog;
    use crate::RegistryPaths;

    #[test]
    fn resolves_enabled_worker_binding() {
        let root = temp_root();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/workers.json"),
            "{\"workers\":[{\"id\":\"coder\",\"kind\":\"jcode\",\"role\":\"implementer\",\"entrypoint\":\"/bin/x\",\"backend\":\"vllm\",\"resource_id\":\"gpu-2060\",\"model_id\":\"coder-model\",\"enabled\":true}]}",
        )
        .unwrap();
        let catalog = FileSystemWorkerCatalog::new(RegistryPaths::new(root));
        let binding = catalog.resolve("coder").unwrap();
        assert_eq!(binding.placement().resource_ids(), ["gpu-2060".to_string()]);
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-worker-catalog-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
