pub struct RootlessFlag;

impl RootlessFlag {
    pub fn assert_enabled(text: &str) -> Result<(), String> {
        if text.trim().eq_ignore_ascii_case("true") {
            return Ok(());
        }
        Err(
            "podman is not running rootless; rootless Podman is required for external-repository command execution"
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::RootlessFlag;

    #[test]
    fn accepts_true() {
        assert!(RootlessFlag::assert_enabled("true\n").is_ok());
    }

    #[test]
    fn rejects_rootful_or_empty() {
        assert!(RootlessFlag::assert_enabled("false").is_err());
        assert!(RootlessFlag::assert_enabled("").is_err());
        assert!(RootlessFlag::assert_enabled("unknown").is_err());
    }
}
