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
    fn list(&self) -> Result<Vec<QueuedTask>, String> {
        fs::create_dir_all(self.paths.queued_dir()).map_err(|error| error.to_string())?;
        let mut tasks = Vec::new();
        for path in read_queue_entries(self.paths.queued_dir())? {
            let task_id = task_id_from_path(&path)?;
            tasks.push(QueuedTask::new(task_id, path_text(&path)?));
        }
        Ok(tasks)
    }

    fn claim(&self, task: &QueuedTask) -> Result<QueuedTask, String> {
        fs::create_dir_all(self.paths.running_dir()).map_err(|error| error.to_string())?;
        let current = PathBuf::from(task.spec_path());
        let target = self
            .paths
            .running_dir()
            .join(format!("{}.json", task.task_id()));
        fs::rename(current, &target).map_err(|error| error.to_string())?;
        Ok(QueuedTask::new(
            task.task_id().to_string(),
            path_text(&target)?,
        ))
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

    fn requeue(&self, task: &QueuedTask) -> Result<QueuedTask, String> {
        let current = PathBuf::from(task.spec_path());
        let target = self
            .paths
            .queued_dir()
            .join(format!("{}.json", task.task_id()));
        fs::rename(current, &target).map_err(|error| error.to_string())?;
        Ok(QueuedTask::new(
            task.task_id().to_string(),
            path_text(&target)?,
        ))
    }
}

fn read_queue_entries(directory: PathBuf) -> Result<Vec<PathBuf>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            entries.push(path);
        }
    }
    entries.sort();
    Ok(entries)
}

fn task_id_from_path(path: &PathBuf) -> Result<String, String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .ok_or("invalid queued task name".to_string())
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
    fn lists_claims_and_completes_task() {
        let root = temp_root();
        let queued_dir = root.join("state/queue/queued");
        fs::create_dir_all(&queued_dir).unwrap();
        fs::write(queued_dir.join("task-a.json"), "{}").unwrap();
        fs::write(queued_dir.join(".gitkeep"), "").unwrap();
        let repository =
            FileSystemExecutionQueueRepository::new(RepositoryPaths::new(root.clone()));
        let queued = repository.list().unwrap();
        let running = repository.claim(&queued[0]).unwrap();
        assert!(PathBuf::from(running.spec_path()).exists());
        repository.complete(&running).unwrap();
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
        let queued = repository.list().unwrap();
        let running = repository.claim(&queued[0]).unwrap();
        let requeued = repository.requeue(&running).unwrap();
        assert!(root.join("state/queue/queued/task-b.json").exists());
        assert!(requeued.spec_path().ends_with("task-b.json"));
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
