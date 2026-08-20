use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct ChangeId(String);

impl ChangeId {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("change id cannot be empty".to_string());
        }
        if !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
        {
            return Err("change id must be filesystem-safe".to_string());
        }
        if trimmed.starts_with('.') || trimmed.ends_with('.') {
            return Err("change id must be filesystem-safe".to_string());
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeId;

    #[test]
    fn rejects_blank_change_id() {
        assert_eq!(
            ChangeId::new("  ".to_string()),
            Err("change id cannot be empty".to_string())
        );
    }

    #[test]
    fn rejects_unsafe_change_id() {
        assert_eq!(
            ChangeId::new("../etc".to_string()),
            Err("change id must be filesystem-safe".to_string())
        );
    }

    #[test]
    fn keeps_valid_change_id() {
        let change_id = ChangeId::new("adaptos-20260820-001".to_string()).unwrap();
        assert_eq!(change_id.value(), "adaptos-20260820-001");
    }
}
