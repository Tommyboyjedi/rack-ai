use rack_ai_domain::RunState;
use rack_ai_domain::TaskId;

pub trait RunStateRepository {
    fn save(&self, run_state: &RunState) -> Result<(), String>;
    fn find(&self, task_id: &TaskId) -> Result<Option<RunState>, String>;
    fn list(&self) -> Result<Vec<RunState>, String>;
}
