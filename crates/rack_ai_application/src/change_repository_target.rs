use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::GitRef;
use rack_ai_domain::GitSha;
use rack_ai_domain::RepositoryId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRepositoryTarget {
    id: RepositoryId,
    registered_root: PathBuf,
    base_ref: GitRef,
    base_sha: GitSha,
}

impl ChangeRepositoryTarget {
    pub fn new(id: RepositoryId, registered_root: PathBuf) -> Result<Self, String> {
        if !registered_root.is_absolute() {
            return Err("registered repository root must be an absolute path".to_string());
        }
        Ok(Self {
            id,
            registered_root,
            base_ref: GitRef::new("main".to_string())?,
            base_sha: GitSha::new("0".repeat(40))?,
        })
    }

    pub fn with_base_ref(mut self, base_ref: GitRef) -> Self {
        self.base_ref = base_ref;
        self
    }

    pub fn with_base_sha(mut self, base_sha: GitSha) -> Self {
        self.base_sha = base_sha;
        self
    }

    pub fn id(&self) -> &RepositoryId {
        &self.id
    }

    pub fn registered_root(&self) -> &Path {
        self.registered_root.as_path()
    }

    pub fn base_ref(&self) -> &GitRef {
        &self.base_ref
    }

    pub fn base_sha(&self) -> &GitSha {
        &self.base_sha
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeRepositoryTarget;
    use rack_ai_domain::GitSha;
    use rack_ai_domain::RepositoryId;
    use std::path::PathBuf;

    #[test]
    fn records_resolved_sha() {
        let target = ChangeRepositoryTarget::new(
            RepositoryId::new("adaptos".to_string()).unwrap(),
            PathBuf::from("/srv/projects/adaptos"),
        )
        .unwrap()
        .with_base_sha(GitSha::new("a".repeat(40)).unwrap());
        assert_eq!(target.base_sha().value(), "a".repeat(40));
    }
}
