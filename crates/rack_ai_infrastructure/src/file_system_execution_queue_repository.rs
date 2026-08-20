use std::fs;
use std::path::PathBuf;

use rack_ai_application::ExecutionQueueRepository;
use rack_ai_application::QueuedTask;

use crate::RepositoryPaths;

pub struct FileSystemExecutionQueueRepository {
    paths: RepositoryPaths,
}

impl FileSystemExecutionQueueRepository {
    pub fn new(paths: RepositoryPaths) -> Self {
        Self { paths }
    }
}

impl ExecutionQueueRepository for FileSystemExecutionQueueRepository {
    fn take_next(&self) -> Result<Option<QueuedTask>, String> {
        fs::create_dir_all(self.paths.queued_dir()).map_err(|error| error.to_string())?;
        fs::create_dir_all(self.paths.running_dir()).map_err(|error| error.to_string())?;
        let mut entries = read_queue_entries(self.paths.queued_dir())?;
        if entries.is_empty() {
            return Ok(None);
        }
        let queued_path = entries.remove(0);
        let content = fs::read_to_string(&queued_path).map_err(|error| error.to_string())?;
        let task_id = queued_path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("invalid queued task name")?
            .to_string();
        let running_path = self
            .paths
            .running_dir()
            .join(queued_path.file_name().ok_or("invalid queued task path")?);
        fs::rename(&queued_path, &running_path).map_err(|error| error.to_string())?;
        fs::write(&running_path, content).map_err(|error| error.to_string())?;
        Ok(Some(QueuedTask::new(task_id, path_text(&running_path)?)))
    }

    fn complete(&self, task: &QueuedTask) -> Result<(), String> {
        fs::create_dir_all(self.paths.history_dir()).map_err(|error| error.to_string())?;
        let current = PathBuf::from(task.spec_path());
        let target = self
            .paths
            .history_dir()
            .join(format!("{}.spec.json", task.task_id()));
        fs::rename(current, target).map_err(|error| error.to_string())
    }

    fn requeue(&self, task: &QueuedTask) -> Result<(), String> {
        let current = PathBuf::from(task.spec_path());
        let target = self
            .paths
            .queued_dir()
            .join(format!("{}.json", task.task_id()));
        fs::rename(current, target).map_err(|error| error.to_string())
    }
}

fn read_queue_entries(directory: PathBuf) -> Result<Vec<PathBuf>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        entries.push(entry.map_err(|error| error.to_string())?.path());
    }
    entries.sort();
    Ok(entries)
}

fn path_text(path: &PathBuf) -> Result<String, String> {
    path.to_str()
        .map(|value| value.to_string())
        .ok_or("path was not valid UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::ExecutionQueueRepository;

    use super::FileSystemExecutionQueueRepository;
    use crate::RepositoryPaths;

    #[test]
    fn moves_queued_task_into_running_then_history() {
        let root = temp_root();
        let queued_dir = root.join("state/queue/queued");
        fs::create_dir_all(&queued_dir).unwrap();
        fs::write(queued_dir.join("task-a.json"), "{}").unwrap();
        let repository =
            FileSystemExecutionQueueRepository::new(RepositoryPaths::new(root.clone()));
        let task = repository.take_next().unwrap().unwrap();
        assert!(PathBuf::from(task.spec_path()).exists());
        repository.complete(&task).unwrap();
        assert!(root.join("state/queue/history/task-a.spec.json").exists());
    }

    #[test]
    fn requeues_running_task_back_to_queue() {
        let root = temp_root();
        let queued_dir = root.join("state/queue/queued");
        fs::create_dir_all(&queued_dir).unwrap();
        fs::write(queued_dir.join("task-b.json"), "{}").unwrap();
        let repository =
            FileSystemExecutionQueueRepository::new(RepositoryPaths::new(root.clone()));
        let task = repository.take_next().unwrap().unwrap();
        repository.requeue(&task).unwrap();
        assert!(root.join("state/queue/queued/task-b.json").exists());
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-execution-queue-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
