use crate::QueuedTask;

pub trait ExecutionQueueRepository {
    fn take_next(&self) -> Result<Option<QueuedTask>, String>;
    fn complete(&self, task: &QueuedTask) -> Result<(), String>;
    fn requeue(&self, task: &QueuedTask) -> Result<(), String>;
}
