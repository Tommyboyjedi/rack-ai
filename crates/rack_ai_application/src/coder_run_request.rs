#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoderRunRequest {
    task: String,
    max_turns: usize,
    timeout_seconds: Option<u32>,
}

impl CoderRunRequest {
    pub fn new(task: String, max_turns: usize) -> Result<Self, String> {
        if task.trim().is_empty() {
            return Err("coder task cannot be empty".to_string());
        }
        if max_turns == 0 {
            return Err("coder max turns must be greater than zero".to_string());
        }
        Ok(Self {
            task,
            max_turns,
            timeout_seconds: None,
        })
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u32) -> Self {
        self.timeout_seconds = Some(timeout_seconds);
        self
    }

    pub fn task(&self) -> &str {
        self.task.as_str()
    }

    pub fn max_turns(&self) -> usize {
        self.max_turns
    }

    pub fn timeout_seconds(&self) -> Option<u32> {
        self.timeout_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::CoderRunRequest;

    #[test]
    fn rejects_empty_task() {
        assert!(CoderRunRequest::new(" ".to_string(), 4).is_err());
    }
}
