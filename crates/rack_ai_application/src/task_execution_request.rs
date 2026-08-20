pub struct TaskExecutionRequest {
    task_id: String,
    source_spec_path: String,
    execution_spec_json: Option<String>,
}

impl TaskExecutionRequest {
    pub fn new(task_id: String, source_spec_path: String) -> Self {
        Self {
            task_id,
            source_spec_path,
            execution_spec_json: None,
        }
    }

    pub fn with_execution_spec_json(mut self, execution_spec_json: String) -> Self {
        self.execution_spec_json = Some(execution_spec_json);
        self
    }

    pub fn task_id(&self) -> &str {
        self.task_id.as_str()
    }

    pub fn source_spec_path(&self) -> &str {
        self.source_spec_path.as_str()
    }

    pub fn execution_spec_json(&self) -> Option<&str> {
        self.execution_spec_json.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::TaskExecutionRequest;

    #[test]
    fn stores_optional_execution_json() {
        let request = TaskExecutionRequest::new("task-1".to_string(), "/tmp/spec.json".to_string())
            .with_execution_spec_json("{}".to_string());
        assert_eq!(request.task_id(), "task-1");
        assert_eq!(request.execution_spec_json(), Some("{}"));
    }
}
