use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::ChangeLayout;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspacePath {
    relative: String,
}

impl WorkspacePath {
    pub fn parse(requested: &str) -> Result<Self, String> {
        let trimmed = requested.trim();
        if trimmed.is_empty() {
            return Err("workspace path cannot be empty".to_string());
        }
        let relative = if trimmed == ChangeLayout::workspace_mount_path() {
            String::new()
        } else if let Some(stripped) =
            trimmed.strip_prefix(&format!("{}/", ChangeLayout::workspace_mount_path()))
        {
            stripped.to_string()
        } else if Path::new(trimmed).is_absolute() {
            return Err("absolute path is outside the workspace".to_string());
        } else {
            trimmed.to_string()
        };
        if relative.is_empty() {
            return Err("workspace path cannot be the workspace root".to_string());
        }
        let mut normalized = PathBuf::new();
        for component in Path::new(&relative).components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => normalized.push(part),
                Component::ParentDir => {
                    if !normalized.pop() {
                        return Err("workspace path escapes the workspace".to_string());
                    }
                }
                _ => return Err("workspace path is invalid".to_string()),
            }
        }
        if normalized.as_os_str().is_empty() {
            return Err("workspace path cannot be the workspace root".to_string());
        }
        Ok(Self {
            relative: normalized.to_string_lossy().to_string(),
        })
    }

    pub fn relative(&self) -> &str {
        self.relative.as_str()
    }

    pub fn container_path(&self) -> String {
        format!("{}/{}", ChangeLayout::workspace_mount_path(), self.relative)
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspacePath;

    #[test]
    fn rejects_escape_and_host_absolute_paths() {
        assert!(WorkspacePath::parse("../secret").is_err());
        assert!(WorkspacePath::parse("/etc/passwd").is_err());
        assert!(WorkspacePath::parse("/workspace/../etc/passwd").is_err());
        assert!(WorkspacePath::parse("").is_err());
    }

    #[test]
    fn accepts_relative_and_workspace_absolute_paths() {
        let relative = WorkspacePath::parse("src/lib.rs").unwrap();
        let mounted = WorkspacePath::parse("/workspace/src/lib.rs").unwrap();
        assert_eq!(relative.relative(), "src/lib.rs");
        assert_eq!(mounted.relative(), "src/lib.rs");
        assert_eq!(relative.container_path(), "/workspace/src/lib.rs");
    }
}
