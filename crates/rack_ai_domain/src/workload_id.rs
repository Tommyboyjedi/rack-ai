use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkloadId(String);

impl WorkloadId {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("workload id cannot be empty".to_string());
        }
        if !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        {
            return Err("workload id contains unsupported characters".to_string());
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::WorkloadId;

    #[test]
    fn rejects_blank_workload_id() {
        assert_eq!(
            WorkloadId::new(" ".to_string()),
            Err("workload id cannot be empty".to_string())
        );
    }

    #[test]
    fn rejects_unsafe_workload_id() {
        assert_eq!(
            WorkloadId::new("bad/id".to_string()),
            Err("workload id contains unsupported characters".to_string())
        );
    }
}
