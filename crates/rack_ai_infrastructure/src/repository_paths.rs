use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryPaths {
    root: PathBuf,
}

impl RepositoryPaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn queued_dir(&self) -> PathBuf {
        self.root.join("state/queue/queued")
    }

    pub fn running_dir(&self) -> PathBuf {
        self.root.join("state/queue/running")
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("state/runs")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RepositoryPaths;

    #[test]
    fn builds_expected_directories() {
        let paths = RepositoryPaths::new(PathBuf::from("/srv/rack-ai"));
        assert!(paths.queued_dir().ends_with("state/queue/queued"));
        assert!(paths.running_dir().ends_with("state/queue/running"));
        assert!(paths.runs_dir().ends_with("state/runs"));
    }
}
