use crate::ReadFileRequest;
use crate::RunCommandRequest;
use crate::WorkspaceExecutionResult;
use crate::WriteFileRequest;

pub trait WorkspaceExecutor {
    fn write_file(&self, request: &WriteFileRequest) -> Result<WorkspaceExecutionResult, String>;
    fn read_file(&self, request: &ReadFileRequest) -> Result<WorkspaceExecutionResult, String>;
    fn run_command(&self, request: &RunCommandRequest) -> Result<WorkspaceExecutionResult, String>;
}
