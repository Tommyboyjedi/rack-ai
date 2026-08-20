use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryPaths {
    root: PathBuf,
}

impl RepositoryPaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
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

    pub fn history_dir(&self) -> PathBuf {
        self.root.join("state/queue/history")
    }

    pub fn leases_dir(&self) -> PathBuf {
        self.root.join("state/resources/leases")
    }

    pub fn changes_dir(&self) -> PathBuf {
        self.root.join("state/changes")
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
        assert!(paths.history_dir().ends_with("state/queue/history"));
        assert!(paths.leases_dir().ends_with("state/resources/leases"));
        assert!(paths.changes_dir().ends_with("state/changes"));
    }
}
