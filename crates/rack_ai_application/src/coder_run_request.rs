#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoderRunRequest {
    task: String,
    max_turns: usize,
}

impl CoderRunRequest {
    pub fn new(task: String, max_turns: usize) -> Result<Self, String> {
        if task.trim().is_empty() {
            return Err("coder task cannot be empty".to_string());
        }
        if max_turns == 0 {
            return Err("coder max turns must be greater than zero".to_string());
        }
        Ok(Self { task, max_turns })
    }

    pub fn task(&self) -> &str {
        self.task.as_str()
    }

    pub fn max_turns(&self) -> usize {
        self.max_turns
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
