use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct GitRef(String);

impl GitRef {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("git ref cannot be empty".to_string());
        }
        if trimmed.contains(char::is_whitespace) || trimmed.contains("..") {
            return Err("git ref is invalid".to_string());
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::GitRef;

    #[test]
    fn rejects_blank_ref() {
        assert_eq!(
            GitRef::new(" ".to_string()),
            Err("git ref cannot be empty".to_string())
        );
    }

    #[test]
    fn rejects_range_ref() {
        assert_eq!(
            GitRef::new("main..other".to_string()),
            Err("git ref is invalid".to_string())
        );
    }

    #[test]
    fn keeps_branch_name() {
        let git_ref = GitRef::new("main".to_string()).unwrap();
        assert_eq!(git_ref.value(), "main");
    }
}
