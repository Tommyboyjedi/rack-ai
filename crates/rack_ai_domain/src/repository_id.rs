use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct RepositoryId(String);

impl RepositoryId {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("repository id cannot be empty".to_string());
        }
        if !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
        {
            return Err("repository id must be filesystem-safe".to_string());
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::RepositoryId;

    #[test]
    fn rejects_blank_repository_id() {
        assert_eq!(
            RepositoryId::new(" ".to_string()),
            Err("repository id cannot be empty".to_string())
        );
    }

    #[test]
    fn keeps_valid_repository_id() {
        let repository_id = RepositoryId::new("adaptos".to_string()).unwrap();
        assert_eq!(repository_id.value(), "adaptos");
    }
}
