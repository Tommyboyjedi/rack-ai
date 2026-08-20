use rack_ai_application::CoderToolRunner;
use rack_ai_application::CoderWorkspaceContext;
use rack_ai_application::ReadFileRequest;
use rack_ai_application::RunCommandRequest;
use rack_ai_application::WorkspaceExecutor;
use rack_ai_application::WorkspacePath;
use rack_ai_application::WriteFileRequest;
use serde_json::Value;

pub struct WorkspaceCoderToolRunner<'a> {
    executor: &'a dyn WorkspaceExecutor,
    context: CoderWorkspaceContext,
}

impl<'a> WorkspaceCoderToolRunner<'a> {
    pub fn new(executor: &'a dyn WorkspaceExecutor, context: CoderWorkspaceContext) -> Self {
        Self { executor, context }
    }
}

impl CoderToolRunner for WorkspaceCoderToolRunner<'_> {
    fn run(&self, name: &str, arguments: &Value) -> Result<String, String> {
        if name == "write" {
            return self.run_write(arguments);
        }
        if name == "read" {
            return self.run_read(arguments);
        }
        if name == "bash" {
            return self.run_bash(arguments);
        }
        Err(format!("Unsupported tool {name}"))
    }
}

impl WorkspaceCoderToolRunner<'_> {
    fn run_write(&self, arguments: &Value) -> Result<String, String> {
        let path = WorkspacePath::parse(read_required_string(arguments, "file_path")?)?;
        if !self.context.allowed_paths().allows(path.relative()) {
            return Err(format!(
                "write path {} is outside allowed_paths",
                path.relative()
            ));
        }
        let content = read_required_string(arguments, "content")?.to_string();
        let result = self.executor.write_file(
            &WriteFileRequest::new(self.context.worktree_path().to_path_buf(), path)
                .with_content(content)
                .with_timeout_seconds(self.context.timeout_seconds()),
        )?;
        Ok(result.evidence().stdout().to_string())
    }

    fn run_read(&self, arguments: &Value) -> Result<String, String> {
        let path = WorkspacePath::parse(read_required_string(arguments, "file_path")?)?;
        let start_line = arguments
            .get("start_line")
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(400);
        let result = self.executor.read_file(
            &ReadFileRequest::new(self.context.worktree_path().to_path_buf(), path)
                .with_range(start_line, limit)
                .with_timeout_seconds(self.context.timeout_seconds()),
        )?;
        Ok(result.content().to_string())
    }

    fn run_bash(&self, arguments: &Value) -> Result<String, String> {
        let command = read_required_string(arguments, "command")?.to_string();
        let result = self.executor.run_command(
            &RunCommandRequest::new(
                self.context.worktree_path().to_path_buf(),
                vec!["/bin/sh".to_string(), "-lc".to_string(), command],
            )?
            .with_timeout_seconds(self.context.timeout_seconds()),
        )?;
        let mut text = result.evidence().stdout().to_string();
        text.push_str(result.evidence().stderr());
        Ok(text.trim().to_string())
    }
}

fn read_required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(format!("missing string field: {key}"))
}

#[cfg(test)]
mod tests {
    use super::WorkspaceCoderToolRunner;
    use rack_ai_application::CoderToolRunner;
    use rack_ai_application::CoderWorkspaceContext;
    use rack_ai_application::CommandEvidence;
    use rack_ai_application::ReadFileRequest;
    use rack_ai_application::RunCommandRequest;
    use rack_ai_application::WorkspaceExecutionResult;
    use rack_ai_application::WorkspaceExecutor;
    use rack_ai_application::WriteFileRequest;
    use rack_ai_domain::AllowedPath;
    use rack_ai_domain::AllowedPaths;
    use serde_json::json;
    use std::path::PathBuf;

    struct FakeExecutor;

    impl WorkspaceExecutor for FakeExecutor {
        fn write_file(
            &self,
            request: &WriteFileRequest,
        ) -> Result<WorkspaceExecutionResult, String> {
            Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
                vec!["write".to_string(), request.path().relative().to_string()],
                0,
            )))
        }

        fn read_file(&self, request: &ReadFileRequest) -> Result<WorkspaceExecutionResult, String> {
            Ok(
                WorkspaceExecutionResult::new(CommandEvidence::new(vec!["read".to_string()], 0))
                    .with_content(request.path().relative().to_string()),
            )
        }

        fn run_command(
            &self,
            request: &RunCommandRequest,
        ) -> Result<WorkspaceExecutionResult, String> {
            Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
                request.argv().to_vec(),
                0,
            )))
        }
    }

    #[test]
    fn rejects_write_outside_allowed_paths() {
        let runner = WorkspaceCoderToolRunner::new(
            &FakeExecutor,
            CoderWorkspaceContext::new(
                PathBuf::from("/tmp/repo"),
                AllowedPaths::new(vec![AllowedPath::new("src".to_string()).unwrap()]).unwrap(),
            ),
        );
        let error = runner
            .run(
                "write",
                &json!({"file_path": "README.md", "content": "nope"}),
            )
            .unwrap_err();
        assert!(error.contains("outside allowed_paths"));
        assert!(
            runner
                .run(
                    "write",
                    &json!({"file_path": "../secret", "content": "nope"}),
                )
                .is_err()
        );
    }

    #[test]
    fn allows_write_inside_policy() {
        let runner = WorkspaceCoderToolRunner::new(
            &FakeExecutor,
            CoderWorkspaceContext::new(
                PathBuf::from("/tmp/repo"),
                AllowedPaths::new(vec![AllowedPath::new("src".to_string()).unwrap()]).unwrap(),
            ),
        );
        assert!(
            runner
                .run(
                    "write",
                    &json!({"file_path": "src/lib.rs", "content": "ok"}),
                )
                .is_ok()
        );
    }
}
