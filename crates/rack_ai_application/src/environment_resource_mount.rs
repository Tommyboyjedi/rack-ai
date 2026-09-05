use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::ChangeLayout;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentResourceMount {
    source_path: PathBuf,
    container_path: PathBuf,
}

impl EnvironmentResourceMount {
    pub fn same_path(source_path: PathBuf) -> Result<Self, String> {
        Self::new(source_path.clone(), source_path)
    }

    pub fn new(source_path: PathBuf, container_path: PathBuf) -> Result<Self, String> {
        assert_absolute_clean_path(source_path.as_path(), "environment resource source path")?;
        assert_absolute_clean_path(
            container_path.as_path(),
            "environment resource container path",
        )?;
        assert_container_path_is_safe(container_path.as_path())?;
        Ok(Self {
            source_path,
            container_path,
        })
    }

    pub fn source_path(&self) -> &Path {
        self.source_path.as_path()
    }

    pub fn container_path(&self) -> &Path {
        self.container_path.as_path()
    }

    pub fn read_only(&self) -> bool {
        true
    }
}

fn assert_absolute_clean_path(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{label} must be an absolute path"));
    }
    for component in path.components() {
        match component {
            Component::CurDir | Component::ParentDir => {
                return Err(format!("{label} must not contain traversal components"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn assert_container_path_is_safe(path: &Path) -> Result<(), String> {
    let reserved = [
        Path::new(ChangeLayout::workspace_mount_path()),
        Path::new(ChangeLayout::build_cache_mount_path()),
        Path::new("/tmp"),
    ];
    if reserved
        .iter()
        .any(|item| path == *item || path.starts_with(item))
    {
        return Err(format!(
            "environment resource container path {} overlaps a reserved executor path",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::EnvironmentResourceMount;
    use std::path::PathBuf;

    #[test]
    fn defaults_to_read_only_same_path_mount() {
        let mount =
            EnvironmentResourceMount::same_path(PathBuf::from("/srv/runtime/.venv")).unwrap();
        assert_eq!(mount.source_path(), PathBuf::from("/srv/runtime/.venv"));
        assert_eq!(mount.container_path(), PathBuf::from("/srv/runtime/.venv"));
        assert!(mount.read_only());
    }

    #[test]
    fn rejects_traversal_and_reserved_container_paths() {
        assert!(EnvironmentResourceMount::same_path(PathBuf::from("../bad")).is_err());
        assert!(
            EnvironmentResourceMount::new(
                PathBuf::from("/srv/runtime/.venv"),
                PathBuf::from("/workspace/runtime")
            )
            .is_err()
        );
    }
}
