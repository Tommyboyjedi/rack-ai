use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct AllowedPath(String);

impl AllowedPath {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("allowed path cannot be empty".to_string());
        }
        if trimmed.starts_with('/') || trimmed.contains('\0') {
            return Err("allowed path must be a relative workspace path".to_string());
        }
        if std::path::Path::new(trimmed)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err("allowed path must be a relative workspace path".to_string());
        }
        Ok(Self(trimmed.trim_end_matches('/').to_string()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }

    pub fn allows(&self, changed_path: &str) -> bool {
        let normalized = changed_path.trim().trim_end_matches('/');
        normalized == self.0 || normalized.starts_with(&format!("{}/", self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::AllowedPath;

    #[test]
    fn rejects_absolute_or_parent_paths() {
        assert!(AllowedPath::new("/etc/passwd".to_string()).is_err());
        assert!(AllowedPath::new("../secret".to_string()).is_err());
        assert!(AllowedPath::new("src/../secret".to_string()).is_err());
    }

    #[test]
    fn matches_exact_and_nested_paths() {
        let src = AllowedPath::new("src/".to_string()).unwrap();
        let cargo = AllowedPath::new("Cargo.toml".to_string()).unwrap();
        assert!(src.allows("src/lib.rs"));
        assert!(src.allows("src"));
        assert!(!src.allows("srcfoo"));
        assert!(cargo.allows("Cargo.toml"));
        assert!(!cargo.allows("Cargo.toml.bak"));
    }
}
