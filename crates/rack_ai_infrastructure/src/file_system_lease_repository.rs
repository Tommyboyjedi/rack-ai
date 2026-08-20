use std::collections::BTreeMap;
use std::fs;

use rack_ai_application::LeaseRepository;
use rack_ai_domain::Placement;
use rack_ai_domain::TaskId;
use serde::Serialize;

use crate::RepositoryPaths;

pub struct FileSystemLeaseRepository {
    paths: RepositoryPaths,
}

#[derive(Serialize)]
struct LeaseRecord<'a> {
    task_id: &'a str,
    resource_id: &'a str,
    worker_ids: &'a [String],
    model_ids: &'a [String],
}

impl FileSystemLeaseRepository {
    pub fn new(paths: RepositoryPaths) -> Self {
        Self { paths }
    }
}

impl LeaseRepository for FileSystemLeaseRepository {
    fn blocked_resources(&self, placement: &Placement) -> Result<Vec<String>, String> {
        let mut blocked = Vec::new();
        for resource_id in placement.resource_ids() {
            if self.lease_path(resource_id).exists() {
                blocked.push(resource_id.clone());
            }
        }
        Ok(blocked)
    }

    fn acquire(
        &self,
        task_id: &TaskId,
        placement: &Placement,
    ) -> Result<BTreeMap<String, String>, String> {
        fs::create_dir_all(self.paths.leases_dir()).map_err(|error| error.to_string())?;
        let mut lease_paths = BTreeMap::new();
        for resource_id in placement.resource_ids() {
            let path = self.lease_path(resource_id);
            if path.exists() {
                return Err(format!("resource busy: {resource_id}"));
            }
            let record = LeaseRecord {
                task_id: task_id.value(),
                resource_id,
                worker_ids: placement.worker_ids(),
                model_ids: placement.model_ids(),
            };
            let json = serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?;
            fs::write(&path, format!("{json}\n")).map_err(|error| error.to_string())?;
            lease_paths.insert(resource_id.clone(), path.to_string_lossy().to_string());
        }
        Ok(lease_paths)
    }

    fn release(&self, placement: &Placement) -> Result<(), String> {
        for resource_id in placement.resource_ids() {
            let path = self.lease_path(resource_id);
            if path.exists() {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
}

impl FileSystemLeaseRepository {
    fn lease_path(&self, resource_id: &str) -> std::path::PathBuf {
        self.paths.leases_dir().join(format!("{resource_id}.json"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::LeaseRepository;
    use rack_ai_domain::Placement;
    use rack_ai_domain::TaskId;

    use super::FileSystemLeaseRepository;
    use crate::RepositoryPaths;

    #[test]
    fn acquires_blocks_and_releases_resource_leases() {
        let root = temp_root();
        let repository = FileSystemLeaseRepository::new(RepositoryPaths::new(root.clone()));
        let placement = Placement::new(
            vec!["local-coder".to_string()],
            vec!["gpu-2060".to_string()],
        )
        .with_models(vec!["qwen25-coder-3b-awq-local-coder".to_string()]);

        let lease_paths = repository
            .acquire(&TaskId::new("task-lease".to_string()).unwrap(), &placement)
            .unwrap();
        let blocked = repository.blocked_resources(&placement).unwrap();
        let lease_json =
            fs::read_to_string(root.join("state/resources/leases/gpu-2060.json")).unwrap();

        assert_eq!(blocked, vec!["gpu-2060".to_string()]);
        assert!(lease_paths.contains_key("gpu-2060"));
        assert!(lease_json.contains("task-lease"));
        assert!(lease_json.contains("local-coder"));

        repository.release(&placement).unwrap();
        assert!(repository.blocked_resources(&placement).unwrap().is_empty());
    }

    #[test]
    fn refuses_to_acquire_busy_resource() {
        let root = temp_root();
        let repository = FileSystemLeaseRepository::new(RepositoryPaths::new(root.clone()));
        let placement = Placement::new(vec!["worker".to_string()], vec!["gpu-4060ti".to_string()]);

        repository
            .acquire(&TaskId::new("first".to_string()).unwrap(), &placement)
            .unwrap();
        let error = repository
            .acquire(&TaskId::new("second".to_string()).unwrap(), &placement)
            .unwrap_err();

        assert!(error.contains("resource busy: gpu-4060ti"));
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-lease-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
