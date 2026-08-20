use rack_ai_domain::Placement;
use rack_ai_domain::TaskId;

pub trait LeaseRepository {
    fn acquire(&self, task_id: &TaskId, placement: &Placement) -> Result<(), String>;
    fn release(&self, task_id: &TaskId) -> Result<(), String>;
}
