use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub struct GitSha(String);

impl GitSha {
    pub fn new(value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.len() != 40 && trimmed.len() != 64 {
            return Err("git sha must be a full 40 or 64 character hex digest".to_string());
        }
        if !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err("git sha must be a full 40 or 64 character hex digest".to_string());
        }
        Ok(Self(trimmed.to_ascii_lowercase()))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::GitSha;

    #[test]
    fn rejects_short_sha() {
        assert_eq!(
            GitSha::new("abc123".to_string()),
            Err("git sha must be a full 40 or 64 character hex digest".to_string())
        );
    }

    #[test]
    fn accepts_full_hex_digest() {
        let sha = GitSha::new("0123456789abcdef0123456789ABCDEF01234567".to_string()).unwrap();
        assert_eq!(sha.value(), "0123456789abcdef0123456789abcdef01234567");
    }
}
