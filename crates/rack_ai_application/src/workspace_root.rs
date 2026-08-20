use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRoot(PathBuf);

impl WorkspaceRoot {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("workspace root must be an absolute path".to_string());
        }
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_path()
    }

    pub fn join(&self, child: &str) -> PathBuf {
        self.0.join(child)
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceRoot;
    use std::path::PathBuf;

    #[test]
    fn rejects_relative_root() {
        assert_eq!(
            WorkspaceRoot::new(PathBuf::from("workspaces")),
            Err("workspace root must be an absolute path".to_string())
        );
    }

    #[test]
    fn keeps_absolute_root() {
        let root = WorkspaceRoot::new(PathBuf::from("/srv/rack-workspaces")).unwrap();
        assert_eq!(root.as_path(), PathBuf::from("/srv/rack-workspaces"));
    }
}
