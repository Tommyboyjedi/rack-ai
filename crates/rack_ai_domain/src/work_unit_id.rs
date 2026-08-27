use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkUnitId(String);

impl WorkUnitId {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("work unit id cannot be empty".to_string());
        }
        if !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        {
            return Err("work unit id contains unsupported characters".to_string());
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::WorkUnitId;

    #[test]
    fn rejects_blank_work_unit_id() {
        assert_eq!(
            WorkUnitId::new(" ".to_string()),
            Err("work unit id cannot be empty".to_string())
        );
    }

    #[test]
    fn rejects_unsafe_work_unit_id() {
        assert_eq!(
            WorkUnitId::new("bad/id".to_string()),
            Err("work unit id contains unsupported characters".to_string())
        );
    }
}
