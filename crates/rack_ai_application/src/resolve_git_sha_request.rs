use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::GitRef;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolveGitShaRequest {
    repository_root: PathBuf,
    git_ref: GitRef,
}

impl ResolveGitShaRequest {
    pub fn new(repository_root: PathBuf, git_ref: GitRef) -> Self {
        Self {
            repository_root,
            git_ref,
        }
    }

    pub fn repository_root(&self) -> &Path {
        self.repository_root.as_path()
    }

    pub fn git_ref(&self) -> &GitRef {
        &self.git_ref
    }
}

#[cfg(test)]
mod tests {
    use super::ResolveGitShaRequest;
    use rack_ai_domain::GitRef;
    use std::path::PathBuf;

    #[test]
    fn stores_root_and_ref() {
        let request = ResolveGitShaRequest::new(
            PathBuf::from("/srv/projects/adaptos"),
            GitRef::new("main".to_string()).unwrap(),
        );
        assert_eq!(request.git_ref().value(), "main");
    }
}
