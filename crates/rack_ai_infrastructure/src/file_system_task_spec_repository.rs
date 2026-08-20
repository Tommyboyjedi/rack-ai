use std::fs;

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
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::TaskSpecRepository;

    use super::FileSystemTaskSpecRepository;
    use crate::RepositoryPaths;

    #[test]
    fn writes_task_spec_into_queue_directory() {
        let root = temp_root();
        let repository = FileSystemTaskSpecRepository::new(RepositoryPaths::new(root.clone()));
        repository.save("task-a", "{}").unwrap();
        let path = root.join("state/queue/queued/task-a.json");
        assert_eq!(fs::read_to_string(path).unwrap(), "{}\n");
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
