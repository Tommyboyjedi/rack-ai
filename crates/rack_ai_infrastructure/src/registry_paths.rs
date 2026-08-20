use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryPaths {
    root: PathBuf,
}

impl RegistryPaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn workers_path(&self) -> PathBuf {
        self.root.join("config/workers.json")
    }

    pub fn resources_path(&self) -> PathBuf {
        self.root.join("config/resources.json")
    }

    pub fn models_path(&self) -> PathBuf {
        self.root.join("config/models.json")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RegistryPaths;

    #[test]
    fn builds_registry_paths() {
        let paths = RegistryPaths::new(PathBuf::from("/srv/rack-ai"));
        assert!(paths.workers_path().ends_with("config/workers.json"));
        assert!(paths.resources_path().ends_with("config/resources.json"));
        assert!(paths.models_path().ends_with("config/models.json"));
    }
}
