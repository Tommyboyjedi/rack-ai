#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallRecord {
    pub name: String,
    pub arguments: String,
    pub result: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementChangeResult {
    output: String,
    // TODO(cleanup): The generic ExecuteChange -> ReviewPacket route currently persists
    // output/errors but drops stderr, parsed tool_calls and executor_kind. Preserve that evidence or remove the dead route data.
    stderr: Option<String>,
    tool_calls: Vec<ToolCallRecord>,
    protocol_error: Option<String>,
    worker_error: Option<String>,
    executor_kind: String,
}

impl ImplementChangeResult {
    pub fn new(output: String) -> Self {
        Self {
            output,
            stderr: None,
            tool_calls: Vec::new(),
            protocol_error: None,
            worker_error: None,
            executor_kind: "workspace".to_string(),
        }
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCallRecord>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    pub fn with_stderr(mut self, stderr: String) -> Self {
        if !stderr.trim().is_empty() {
            self.stderr = Some(stderr);
        }
        self
    }

    pub fn with_protocol_error(mut self, protocol_error: String) -> Self {
        self.protocol_error = Some(protocol_error);
        self
    }

    pub fn with_worker_error(mut self, worker_error: String) -> Self {
        self.worker_error = Some(worker_error);
        self
    }

    pub fn with_executor_kind(mut self, executor_kind: String) -> Self {
        self.executor_kind = executor_kind;
        self
    }

    pub fn output(&self) -> &str {
        self.output.as_str()
    }

    pub fn stderr(&self) -> Option<&str> {
        self.stderr.as_deref()
    }

    pub fn tool_calls(&self) -> &[ToolCallRecord] {
        self.tool_calls.as_slice()
    }

    pub fn protocol_error(&self) -> Option<&str> {
        self.protocol_error.as_deref()
    }

    pub fn worker_error(&self) -> Option<&str> {
        self.worker_error.as_deref()
    }

    pub fn executor_kind(&self) -> &str {
        self.executor_kind.as_str()
    }

    pub fn used_host_shell(&self) -> bool {
        self.executor_kind == "host"
    }
}

#[cfg(test)]
mod tests {
    use super::ImplementChangeResult;

    #[test]
    fn stores_model_output() {
        let result = ImplementChangeResult::new("COMPLETE".to_string());
        assert_eq!(result.output(), "COMPLETE");
    }
}
