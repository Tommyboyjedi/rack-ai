use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct TaskId(String);

impl TaskId {
    pub fn new(value: String) -> Result<Self, String> {
        if value.trim().is_empty() {
            return Err("task id cannot be empty".to_string());
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::TaskId;

    #[test]
    fn rejects_blank_task_id() {
        let result = TaskId::new("   ".to_string());
        assert_eq!(result, Err("task id cannot be empty".to_string()));
    }

    #[test]
    fn keeps_valid_task_id() {
        let task_id = TaskId::new("task-123".to_string()).unwrap();
        assert_eq!(task_id.value(), "task-123");
    }
}
