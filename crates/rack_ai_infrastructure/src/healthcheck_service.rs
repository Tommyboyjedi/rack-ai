use std::collections::HashMap;

use crate::EndpointProbe;
use crate::FileSystemRegistryRepository;
use crate::HealthcheckEntry;
use crate::HealthcheckSnapshot;

pub struct HealthcheckService<'a> {
    endpoint_probe: &'a EndpointProbe,
    registry_repository: &'a FileSystemRegistryRepository,
}

impl<'a> HealthcheckService<'a> {
    pub fn new(dependencies: HealthcheckServiceDependencies<'a>) -> Self {
        Self {
            endpoint_probe: dependencies.endpoint_probe,
            registry_repository: dependencies.registry_repository,
        }
    }

    pub fn execute(&self) -> Result<HealthcheckSnapshot, String> {
        let workers = self.registry_repository.load_workers()?;
        let resources = self.registry_repository.load_resources()?;
        let models = self.registry_repository.load_models()?;
        let resource_by_id = resources
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect::<HashMap<_, _>>();
        let model_by_id = models
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect::<HashMap<_, _>>();
        let mut ok = true;
        let mut checks = Vec::new();
        for worker in workers {
            let resource = resource_by_id.get(worker.resource_id.as_str());
            let model = model_by_id.get(worker.model_id.as_str());
            let endpoint_ok = if let Some(active_model) = model {
                if active_model.status == "active" {
                    Some(
                        self.endpoint_probe
                            .check_models(active_model.endpoint.as_str())?,
                    )
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(value) = endpoint_ok {
                ok = ok && value;
            }
            checks.push(HealthcheckEntry {
                worker_id: worker.id,
                enabled: worker.enabled,
                backend: worker.backend,
                resource_id: worker.resource_id.clone(),
                resource_status: resource.map(|item| item.status.clone()),
                model_id: worker.model_id.clone(),
                model_status: model.map(|item| item.status.clone()),
                endpoint_ok,
            });
        }
        Ok(HealthcheckSnapshot { ok, checks })
    }
}

pub struct HealthcheckServiceDependencies<'a> {
    pub endpoint_probe: &'a EndpointProbe,
    pub registry_repository: &'a FileSystemRegistryRepository,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::HealthcheckService;
    use super::HealthcheckServiceDependencies;
    use crate::EndpointProbe;
    use crate::FileSystemRegistryRepository;
    use crate::RegistryPaths;

    #[test]
    fn returns_checks_for_registry_workers() {
        let root = temp_root();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(root.join("config/workers.json"), "{\"workers\":[{\"id\":\"w\",\"kind\":\"jcode\",\"role\":\"planner\",\"entrypoint\":\"/bin/x\",\"backend\":\"jcode\",\"resource_id\":\"r\",\"model_id\":\"m\",\"enabled\":true}]}").unwrap();
        fs::write(root.join("config/resources.json"), "{\"resources\":[{\"id\":\"r\",\"type\":\"gpu\",\"label\":\"GPU\",\"vram_gb\":16,\"device_hint\":\"planner\",\"max_concurrent_tasks\":1,\"owner\":\"w\",\"status\":\"active\"}]}").unwrap();
        fs::write(root.join("config/models.json"), "{\"models\":[{\"id\":\"m\",\"label\":\"Model\",\"role\":\"planner\",\"backend\":\"vllm\",\"worker_id\":\"w\",\"endpoint\":\"http://127.0.0.1:9999/v1\",\"port\":9999,\"status\":\"inactive\"}]}").unwrap();
        let repository = FileSystemRegistryRepository::new(RegistryPaths::new(root));
        let probe = EndpointProbe;
        let service = HealthcheckService::new(HealthcheckServiceDependencies {
            endpoint_probe: &probe,
            registry_repository: &repository,
        });
        let snapshot = service.execute().unwrap();
        assert_eq!(snapshot.checks.len(), 1);
        assert!(snapshot.ok);
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-healthcheck-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
