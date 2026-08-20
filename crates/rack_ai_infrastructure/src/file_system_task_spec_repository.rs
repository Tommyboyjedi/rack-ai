use std::fs;

use rack_ai_application::QueuedTask;
use rack_ai_application::TaskSpec;
use rack_ai_application::TaskSpecRepository;

use crate::RepositoryPaths;

pub struct FileSystemTaskSpecRepository {
    paths: RepositoryPaths,
}

impl FileSystemTaskSpecRepository {
    pub fn new(paths: RepositoryPaths) -> Self {
        Self { paths }
    }
}

impl TaskSpecRepository for FileSystemTaskSpecRepository {
    fn save(&self, task_id: &str, spec_json: &str) -> Result<(), String> {
        let queued_dir = self.paths.queued_dir();
        fs::create_dir_all(&queued_dir).map_err(|error| error.to_string())?;
        let path = queued_dir.join(format!("{task_id}.json"));
        fs::write(path, format!("{spec_json}\n")).map_err(|error| error.to_string())
    }

    fn load(&self, task: &QueuedTask) -> Result<TaskSpec, String> {
        let content = fs::read_to_string(task.spec_path()).map_err(|error| error.to_string())?;
        serde_json::from_str(&content).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::QueuedTask;
    use rack_ai_application::TaskSpecRepository;

    use super::FileSystemTaskSpecRepository;
    use crate::RepositoryPaths;

    #[test]
    fn writes_task_spec_into_queue_directory() {
        let root = temp_root();
        let repository = FileSystemTaskSpecRepository::new(RepositoryPaths::new(root.clone()));
        repository
            .save(
                "task-a",
                "{\"task_id\":\"task-a\",\"placement\":{\"worker_ids\":[],\"resource_ids\":[],\"model_ids\":[],\"backends\":[]}}",
            )
            .unwrap();
        let path = root.join("state/queue/queued/task-a.json");
        assert!(path.exists());
    }

    #[test]
    fn loads_task_spec_from_running_path() {
        let root = temp_root();
        let running_dir = root.join("state/queue/running");
        fs::create_dir_all(&running_dir).unwrap();
        let path = running_dir.join("task-a.json");
        fs::write(&path, "{\"task_id\":\"task-a\",\"placement\":{\"worker_ids\":[],\"resource_ids\":[],\"model_ids\":[],\"backends\":[]}}\n").unwrap();
        let repository = FileSystemTaskSpecRepository::new(RepositoryPaths::new(root));
        let spec = repository
            .load(&QueuedTask::new(
                "task-a".to_string(),
                path.to_str().unwrap().to_string(),
            ))
            .unwrap();
        assert!(!spec.has_dag());
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-task-spec-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
