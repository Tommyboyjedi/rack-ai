use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::GitRef;
use rack_ai_domain::RepositoryId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredRepository {
    id: RepositoryId,
    root: PathBuf,
    default_base_ref: GitRef,
    enabled: bool,
}

impl RegisteredRepository {
    pub fn new(id: RepositoryId, root: PathBuf) -> Result<Self, String> {
        if !root.is_absolute() {
            return Err("registered repository root must be an absolute path".to_string());
        }
        Ok(Self {
            id,
            root,
            default_base_ref: GitRef::new("main".to_string())?,
            enabled: true,
        })
    }

    pub fn with_default_base_ref(mut self, default_base_ref: GitRef) -> Self {
        self.default_base_ref = default_base_ref;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn id(&self) -> &RepositoryId {
        &self.id
    }

    pub fn root(&self) -> &Path {
        self.root.as_path()
    }

    pub fn default_base_ref(&self) -> &GitRef {
        &self.default_base_ref
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::RegisteredRepository;
    use rack_ai_domain::RepositoryId;
    use std::path::PathBuf;

    #[test]
    fn rejects_relative_root() {
        let id = RepositoryId::new("adaptos".to_string()).unwrap();
        assert!(RegisteredRepository::new(id, PathBuf::from("relative")).is_err());
    }

    #[test]
    fn stores_absolute_root() {
        let id = RepositoryId::new("adaptos".to_string()).unwrap();
        let repo = RegisteredRepository::new(id, PathBuf::from("/srv/projects/adaptos")).unwrap();
        assert_eq!(repo.root(), PathBuf::from("/srv/projects/adaptos"));
        assert!(repo.enabled());
    }
}
