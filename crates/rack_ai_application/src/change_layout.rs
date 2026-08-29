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
        16
    }

    pub fn is_ephemeral_path(path: &str) -> bool {
        let normalized = path.trim_end_matches('/');
        normalized == "target"
            || normalized.starts_with("target/")
            || normalized == ".rack-cargo"
            || normalized.starts_with(".rack-cargo/")
            || contains_path_segment(normalized, "__pycache__")
            || contains_path_segment(normalized, ".pytest_cache")
    }
}

fn contains_path_segment(path: &str, segment: &str) -> bool {
    path == segment
        || path.starts_with(&format!("{segment}/"))
        || path.contains(&format!("/{segment}/"))
        || path.ends_with(&format!("/{segment}"))
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
        assert_eq!(ChangeLayout::coder_max_turns(), 16);
        assert!(ChangeLayout::is_ephemeral_path("target/debug/fixture"));
        assert!(ChangeLayout::is_ephemeral_path("__pycache__/"));
        assert!(ChangeLayout::is_ephemeral_path("tests/__pycache__/"));
        assert!(ChangeLayout::is_ephemeral_path(
            ".pytest_cache/v/cache/nodeids"
        ));
        assert!(!ChangeLayout::is_ephemeral_path("src/lib.rs"));
    }
}
