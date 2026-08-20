use crate::TaskExecution;
use crate::TaskExecutionRequest;

pub trait TaskExecutor {
    fn execute(&self, request: &TaskExecutionRequest) -> Result<TaskExecution, String>;
}
