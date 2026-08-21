use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeTask(String);

impl ChangeTask {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("change task cannot be empty".to_string());
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeTask;

    #[test]
    fn rejects_blank_task() {
        assert_eq!(
            ChangeTask::new("\n".to_string()),
            Err("change task cannot be empty".to_string())
        );
    }

    #[test]
    fn keeps_task_text() {
        let task = ChangeTask::new("Add a bounded feature with tests.".to_string()).unwrap();
        assert_eq!(task.value(), "Add a bounded feature with tests.");
    }
}
