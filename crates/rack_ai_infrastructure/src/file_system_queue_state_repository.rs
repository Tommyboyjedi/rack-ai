use std::fs;
use std::path::PathBuf;

use rack_ai_application::QueueStateRepository;

use crate::RepositoryPaths;

pub struct FileSystemQueueStateRepository {
    paths: RepositoryPaths,
}

impl FileSystemQueueStateRepository {
    pub fn new(paths: RepositoryPaths) -> Self {
        Self { paths }
    }
}

impl QueueStateRepository for FileSystemQueueStateRepository {
    fn queued_entries(&self) -> Result<Vec<String>, String> {
        read_names(self.paths.queued_dir())
    }

    fn running_entries(&self) -> Result<Vec<String>, String> {
        read_names(self.paths.running_dir())
    }
}

fn read_names(directory: PathBuf) -> Result<Vec<String>, String> {
    if !directory.exists() {
        return Ok(vec![]);
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::QueueStateRepository;

    use super::FileSystemQueueStateRepository;
    use crate::RepositoryPaths;

    #[test]
    fn lists_queued_and_running_entries() {
        let root = temp_root();
        fs::create_dir_all(root.join("state/queue/queued")).unwrap();
        fs::create_dir_all(root.join("state/queue/running")).unwrap();
        fs::write(root.join("state/queue/queued/a.json"), "{}").unwrap();
        fs::write(root.join("state/queue/queued/.gitkeep"), "").unwrap();
        fs::write(root.join("state/queue/running/b.json"), "{}").unwrap();
        let repository = FileSystemQueueStateRepository::new(RepositoryPaths::new(root));
        assert_eq!(
            repository.queued_entries().unwrap(),
            vec!["a.json".to_string()]
        );
        assert_eq!(
            repository.running_entries().unwrap(),
            vec!["b.json".to_string()]
        );
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-queue-state-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
