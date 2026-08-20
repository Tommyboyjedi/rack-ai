use crate::QueuedTask;
use crate::TaskSpec;

pub trait TaskSpecRepository {
    fn save(&self, task_id: &str, spec_json: &str) -> Result<(), String>;
    fn load(&self, task: &QueuedTask) -> Result<TaskSpec, String>;
}
