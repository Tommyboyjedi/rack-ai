use std::fs;

use rack_ai_application::RunStateRepository;
use rack_ai_domain::RunState;
use rack_ai_domain::TaskId;

use crate::RepositoryPaths;

pub struct FileSystemRunStateRepository {
    paths: RepositoryPaths,
}

impl FileSystemRunStateRepository {
    pub fn new(paths: RepositoryPaths) -> Self {
        Self { paths }
    }
}

impl RunStateRepository for FileSystemRunStateRepository {
    fn save(&self, run_state: &RunState) -> Result<(), String> {
        let runs_dir = self.paths.runs_dir();
        fs::create_dir_all(&runs_dir).map_err(|error| error.to_string())?;
        let path = runs_dir.join(format!("{}.json", run_state.task_id().value()));
        let json = serde_json::to_string_pretty(run_state).map_err(|error| error.to_string())?;
        fs::write(path, format!("{json}\n")).map_err(|error| error.to_string())
    }

    fn find(&self, task_id: &TaskId) -> Result<Option<RunState>, String> {
        let path = self
            .paths
            .runs_dir()
            .join(format!("{}.json", task_id.value()));
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        let run_state = serde_json::from_str(&content).map_err(|error| error.to_string())?;
        Ok(Some(run_state))
    }

    fn list(&self) -> Result<Vec<RunState>, String> {
        let runs_dir = self.paths.runs_dir();
        if !runs_dir.exists() {
            return Ok(vec![]);
        }
        let mut items: Vec<RunState> = Vec::new();
        for entry in fs::read_dir(runs_dir).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if !is_json_file(&path) {
                continue;
            }
            let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
            items.push(serde_json::from_str(&content).map_err(|error| error.to_string())?);
        }
        items.sort_by(|left, right| left.task_id().value().cmp(right.task_id().value()));
        Ok(items)
    }
}

fn is_json_file(path: &std::path::Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("json")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::RunStateRepository;
    use rack_ai_domain::AttemptLimit;
    use rack_ai_domain::Placement;
    use rack_ai_domain::RunState;
    use rack_ai_domain::RunStateDraft;
    use rack_ai_domain::TaskId;
    use rack_ai_domain::TimeoutSeconds;

    use super::FileSystemRunStateRepository;
    use crate::RepositoryPaths;

    #[test]
    fn saves_and_reads_run_state() {
        let root = temp_root();
        let repository = FileSystemRunStateRepository::new(RepositoryPaths::new(root));
        let run_state = sample_run_state("task-a");
        repository.save(&run_state).unwrap();
        let found = repository.find(run_state.task_id()).unwrap().unwrap();
        assert_eq!(found, run_state);
    }

    #[test]
    fn lists_saved_run_states() {
        let root = temp_root();
        let repository = FileSystemRunStateRepository::new(RepositoryPaths::new(root.clone()));
        repository.save(&sample_run_state("task-b")).unwrap();
        repository.save(&sample_run_state("task-a")).unwrap();
        fs::write(root.join("state/runs/.gitkeep"), "").unwrap();
        assert_eq!(repository.list().unwrap().len(), 2);
    }

    fn sample_run_state(task_id: &str) -> RunState {
        RunState::queued(RunStateDraft {
            task_id: TaskId::new(task_id.to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(1).unwrap(),
            timeout_seconds: TimeoutSeconds::new(90).unwrap(),
            placement: Placement::new(vec!["worker".to_string()], vec!["gpu".to_string()]),
        })
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-run-state-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
