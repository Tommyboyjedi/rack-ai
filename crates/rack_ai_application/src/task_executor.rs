use crate::QueuedTask;
use crate::TaskExecution;

pub trait TaskExecutor {
    fn execute(&self, task: &QueuedTask) -> Result<TaskExecution, String>;
}
