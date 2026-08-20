pub struct TaskExecution {
    success: bool,
    result_path: Option<String>,
    last_error: Option<String>,
}

impl TaskExecution {
    pub fn success(result_path: Option<String>) -> Self {
        Self {
            success: true,
            result_path,
            last_error: None,
        }
    }

    pub fn failure(last_error: String, result_path: Option<String>) -> Self {
        Self {
            success: false,
            result_path,
            last_error: Some(last_error),
        }
    }

    pub fn was_successful(&self) -> bool {
        self.success
    }
    pub fn result_path(&self) -> Option<&String> {
        self.result_path.as_ref()
    }
    pub fn last_error(&self) -> Option<&String> {
        self.last_error.as_ref()
    }
}
