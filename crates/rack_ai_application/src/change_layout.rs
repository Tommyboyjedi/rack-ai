use std::path::PathBuf;

use rack_ai_domain::ChangeId;

use crate::WorkspaceRoot;

pub struct ChangeLayout;

impl ChangeLayout {
    pub fn branch_name(change_id: &ChangeId) -> String {
        format!("rack/change-{}", change_id.value())
    }

    pub fn worktree_path(workspace_root: &WorkspaceRoot, change_id: &ChangeId) -> PathBuf {
        workspace_root.join(change_id.value()).join("repo")
    }

    pub fn workspace_mount_path() -> &'static str {
        "/workspace"
    }

    pub fn build_cache_mount_path() -> &'static str {
        "/rack-build"
    }

    pub fn coder_max_turns() -> usize {
        8
    }

    pub fn is_ephemeral_path(path: &str) -> bool {
        path == "target"
            || path.starts_with("target/")
            || path == ".rack-cargo"
            || path.starts_with(".rack-cargo/")
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeLayout;
    use crate::WorkspaceRoot;
    use rack_ai_domain::ChangeId;
    use std::path::PathBuf;

    #[test]
    fn builds_branch_and_worktree_locations() {
        let change_id = ChangeId::new("adaptos-001".to_string()).unwrap();
        let root = WorkspaceRoot::new(PathBuf::from("/srv/rack-workspaces")).unwrap();
        assert_eq!(
            ChangeLayout::branch_name(&change_id),
            "rack/change-adaptos-001"
        );
        assert_eq!(
            ChangeLayout::worktree_path(&root, &change_id),
            PathBuf::from("/srv/rack-workspaces/adaptos-001/repo")
        );
        assert!(ChangeLayout::is_ephemeral_path("target/debug/fixture"));
        assert!(!ChangeLayout::is_ephemeral_path("src/lib.rs"));
    }
}
