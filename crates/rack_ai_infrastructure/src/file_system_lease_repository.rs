use std::collections::BTreeMap;
use std::fs;

use rack_ai_application::LeaseRepository;
use rack_ai_application::LeaseState;
use rack_ai_application::LeaseStateRepository;
use rack_ai_domain::Placement;
use rack_ai_domain::TaskId;
use serde::Deserialize;
use serde::Serialize;

use crate::RepositoryPaths;

pub struct FileSystemLeaseRepository {
    paths: RepositoryPaths,
}

#[derive(Deserialize, Serialize)]
struct LeaseRecord {
    task_id: Option<String>,
    resource_id: String,
    #[serde(default)]
    worker_ids: Vec<String>,
    #[serde(default)]
    model_ids: Vec<String>,
    acquired_at: Option<String>,
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
        acquired_at: &str,
    ) -> Result<BTreeMap<String, String>, String> {
        fs::create_dir_all(self.paths.leases_dir()).map_err(|error| error.to_string())?;
        let mut lease_paths = BTreeMap::new();
        for resource_id in placement.resource_ids() {
            let path = self.lease_path(resource_id);
            if path.exists() {
                return Err(format!("resource busy: {resource_id}"));
            }
            let record = LeaseRecord {
                task_id: Some(task_id.value().to_string()),
                resource_id: resource_id.clone(),
                worker_ids: placement.worker_ids().to_vec(),
                model_ids: placement.model_ids().to_vec(),
                acquired_at: Some(acquired_at.to_string()),
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

impl LeaseStateRepository for FileSystemLeaseRepository {
    fn list(&self) -> Result<Vec<LeaseState>, String> {
        let leases_dir = self.paths.leases_dir();
        if !leases_dir.exists() {
            return Ok(vec![]);
        }
        let mut items = Vec::new();
        for entry in fs::read_dir(leases_dir).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let mut record: LeaseRecord =
                serde_json::from_str(&content).map_err(|error| error.to_string())?;
            if record.resource_id.trim().is_empty() {
                record.resource_id = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_string();
            }
            items.push(LeaseState::new(
                record.resource_id,
                record.task_id,
                record.worker_ids,
                record.model_ids,
                record.acquired_at,
                path.to_string_lossy().to_string(),
            ));
        }
        items.sort_by(|left, right| left.resource_id().cmp(right.resource_id()));
        Ok(items)
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
    use rack_ai_application::LeaseStateRepository;
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
            .acquire(
                &TaskId::new("task-lease".to_string()).unwrap(),
                &placement,
                "2026-08-20T21:00:00Z",
            )
            .unwrap();
        let blocked = repository.blocked_resources(&placement).unwrap();
        let lease_json =
            fs::read_to_string(root.join("state/resources/leases/gpu-2060.json")).unwrap();

        assert_eq!(blocked, vec!["gpu-2060".to_string()]);
        assert!(lease_paths.contains_key("gpu-2060"));
        assert!(lease_json.contains("task-lease"));
        assert!(lease_json.contains("local-coder"));
        assert!(lease_json.contains("2026-08-20T21:00:00Z"));

        repository.release(&placement).unwrap();
        assert!(repository.blocked_resources(&placement).unwrap().is_empty());
    }

    #[test]
    fn refuses_to_acquire_busy_resource() {
        let root = temp_root();
        let repository = FileSystemLeaseRepository::new(RepositoryPaths::new(root.clone()));
        let placement = Placement::new(vec!["worker".to_string()], vec!["gpu-4060ti".to_string()]);

        repository
            .acquire(
                &TaskId::new("first".to_string()).unwrap(),
                &placement,
                "2026-08-20T21:00:00Z",
            )
            .unwrap();
        let error = repository
            .acquire(
                &TaskId::new("second".to_string()).unwrap(),
                &placement,
                "2026-08-20T21:01:00Z",
            )
            .unwrap_err();

        assert!(error.contains("resource busy: gpu-4060ti"));
    }

    #[test]
    fn lists_existing_leases() {
        let root = temp_root();
        let repository = FileSystemLeaseRepository::new(RepositoryPaths::new(root.clone()));
        fs::create_dir_all(root.join("state/resources/leases")).unwrap();
        fs::write(
            root.join("state/resources/leases/gpu-2060.json"),
            "{\n  \"task_id\": \"task-1\",\n  \"resource_id\": \"gpu-2060\",\n  \"worker_ids\": [\"local-coder\"],\n  \"model_ids\": [\"coder-model\"],\n  \"acquired_at\": \"2026-08-20T21:10:00Z\"\n}\n",
        )
        .unwrap();

        let leases = repository.list().unwrap();

        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].resource_id(), "gpu-2060");
        assert_eq!(leases[0].task_id(), Some(&"task-1".to_string()));
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
