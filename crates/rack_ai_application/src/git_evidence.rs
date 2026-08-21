use rack_ai_domain::GitSha;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitEvidence {
    head_sha: GitSha,
    status: String,
    diff: String,
    diff_stat: String,
    changed_paths: Vec<String>,
}

impl GitEvidence {
    pub fn new(head_sha: GitSha, status: String) -> Self {
        Self {
            head_sha,
            status,
            diff: String::new(),
            diff_stat: String::new(),
            changed_paths: Vec::new(),
        }
    }

    pub fn with_diff(mut self, diff: String) -> Self {
        self.diff = diff;
        self
    }

    pub fn with_diff_stat(mut self, diff_stat: String) -> Self {
        self.diff_stat = diff_stat;
        self
    }

    pub fn with_changed_paths(mut self, changed_paths: Vec<String>) -> Self {
        self.changed_paths = changed_paths;
        self
    }

    pub fn head_sha(&self) -> &GitSha {
        &self.head_sha
    }

    pub fn status(&self) -> &str {
        self.status.as_str()
    }

    pub fn diff(&self) -> &str {
        self.diff.as_str()
    }

    pub fn diff_stat(&self) -> &str {
        self.diff_stat.as_str()
    }

    pub fn changed_paths(&self) -> &[String] {
        self.changed_paths.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::GitEvidence;
    use rack_ai_domain::GitSha;

    #[test]
    fn records_changed_paths() {
        let evidence = GitEvidence::new(
            GitSha::new("e".repeat(40)).unwrap(),
            "?? src/lib.rs".to_string(),
        )
        .with_changed_paths(vec!["src/lib.rs".to_string()]);
        assert_eq!(evidence.changed_paths(), ["src/lib.rs"]);
    }
}
