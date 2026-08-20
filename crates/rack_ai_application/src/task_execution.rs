pub struct TaskExecution {
    success: bool,
}

impl TaskExecution {
    pub fn success() -> Self {
        Self { success: true }
    }

    pub fn failure() -> Self {
        Self { success: false }
    }

    pub fn was_successful(&self) -> bool {
        self.success
    }
}
