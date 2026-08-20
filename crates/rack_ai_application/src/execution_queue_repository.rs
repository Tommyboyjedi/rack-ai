use crate::QueuedTask;

pub trait ExecutionQueueRepository {
    fn list(&self) -> Result<Vec<QueuedTask>, String>;
    fn claim(&self, task: &QueuedTask) -> Result<QueuedTask, String>;
    fn complete(&self, task: &QueuedTask) -> Result<(), String>;
    fn requeue(&self, task: &QueuedTask) -> Result<(), String>;
}
