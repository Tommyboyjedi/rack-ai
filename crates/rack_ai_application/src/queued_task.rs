pub struct QueuedTask {
    task_id: String,
    spec_path: String,
}

impl QueuedTask {
    pub fn new(task_id: String, spec_path: String) -> Self {
        Self { task_id, spec_path }
    }

    pub fn task_id(&self) -> &str {
        self.task_id.as_str()
    }

    pub fn spec_path(&self) -> &str {
        self.spec_path.as_str()
    }
}
