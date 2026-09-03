use std::fs;

use crate::ModelRecord;
use crate::ModelsDocument;
use crate::RegistryPaths;
use crate::ResourceRecord;
use crate::ResourcesDocument;
use crate::WorkerRecord;
use crate::WorkersDocument;

pub struct FileSystemRegistryRepository {
    paths: RegistryPaths,
}

impl FileSystemRegistryRepository {
    pub fn new(paths: RegistryPaths) -> Self {
        Self { paths }
    }

    pub fn load_workers(&self) -> Result<Vec<WorkerRecord>, String> {
        let content =
            fs::read_to_string(self.paths.workers_path()).map_err(|error| error.to_string())?;
        let document =
            serde_json::from_str::<WorkersDocument>(&content).map_err(|error| error.to_string())?;
        Ok(document.workers)
    }

    pub fn load_resources(&self) -> Result<Vec<ResourceRecord>, String> {
        let content =
            fs::read_to_string(self.paths.resources_path()).map_err(|error| error.to_string())?;
        let document = serde_json::from_str::<ResourcesDocument>(&content)
            .map_err(|error| error.to_string())?;
        Ok(document.resources)
    }

    pub fn load_models(&self) -> Result<Vec<ModelRecord>, String> {
        let content =
            fs::read_to_string(self.paths.models_path()).map_err(|error| error.to_string())?;
        let document =
            serde_json::from_str::<ModelsDocument>(&content).map_err(|error| error.to_string())?;
        Ok(document.models)
    }

    pub fn load_source_admission_policies(
        &self,
    ) -> Result<Vec<rack_ai_application::GenericSourceAdmissionPolicy>, String> {
        let content =
            fs::read_to_string(self.paths.models_path()).map_err(|error| error.to_string())?;
        let document =
            serde_json::from_str::<ModelsDocument>(&content).map_err(|error| error.to_string())?;
        Ok(document.source_admission_policies)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::FileSystemRegistryRepository;
    use crate::RegistryPaths;

    #[test]
    fn loads_registry_documents() {
        let root = temp_root();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(root.join("config/workers.json"), "{\"workers\":[{\"id\":\"w\",\"kind\":\"jcode\",\"role\":\"planner\",\"entrypoint\":\"/bin/x\",\"backend\":\"jcode\",\"resource_id\":\"r\",\"model_id\":\"m\",\"enabled\":true}]}").unwrap();
        fs::write(root.join("config/resources.json"), "{\"resources\":[{\"id\":\"r\",\"type\":\"gpu\",\"label\":\"GPU\",\"vram_gb\":16,\"device_hint\":\"planner\",\"max_concurrent_tasks\":1,\"owner\":\"w\",\"status\":\"active\"}]}").unwrap();
        fs::write(root.join("config/models.json"), "{\"models\":[{\"id\":\"m\",\"label\":\"Model\",\"role\":\"planner\",\"backend\":\"vllm\",\"worker_id\":\"w\",\"endpoint\":\"http://127.0.0.1:9999/v1\",\"port\":9999,\"status\":\"active\"}]}").unwrap();
        let repository = FileSystemRegistryRepository::new(RegistryPaths::new(root));
        assert_eq!(repository.load_workers().unwrap().len(), 1);
        assert_eq!(repository.load_resources().unwrap().len(), 1);
        assert_eq!(repository.load_models().unwrap().len(), 1);
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-registry-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
