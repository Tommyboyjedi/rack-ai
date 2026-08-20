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
