use std::collections::BTreeMap;

use rack_ai_domain::Placement;
use rack_ai_domain::TaskId;

pub trait LeaseRepository {
    fn blocked_resources(&self, placement: &Placement) -> Result<Vec<String>, String>;
    fn acquire(
        &self,
        task_id: &TaskId,
        placement: &Placement,
    ) -> Result<BTreeMap<String, String>, String>;
    fn release(&self, placement: &Placement) -> Result<(), String>;
}
