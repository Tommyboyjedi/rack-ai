use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RequiredArtifact(String);

impl RequiredArtifact {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("required artifact path cannot be empty".to_string());
        }
        if trimmed.starts_with('/')
            || std::path::Path::new(trimmed)
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err("required artifact must be a relative workspace path".to_string());
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::RequiredArtifact;

    #[test]
    fn rejects_absolute_artifact() {
        assert!(RequiredArtifact::new("/tmp/out".to_string()).is_err());
    }

    #[test]
    fn keeps_relative_artifact() {
        let artifact = RequiredArtifact::new("target/evidence.txt".to_string()).unwrap();
        assert_eq!(artifact.value(), "target/evidence.txt");
    }
}
