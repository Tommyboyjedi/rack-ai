use serde::Deserialize;
use serde::Serialize;

use crate::ExecutorRecord;
use crate::RepositoryRecord;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositoriesDocument {
    pub workspace_root: String,
    #[serde(default)]
    pub approved_programs: Vec<String>,
    pub executor: ExecutorRecord,
    #[serde(default)]
    pub repositories: Vec<RepositoryRecord>,
}

#[cfg(test)]
mod tests {
    use super::RepositoriesDocument;

    #[test]
    fn parses_registry_document() {
        let document = serde_json::from_str::<RepositoriesDocument>(
            r#"{
                "workspace_root": "/srv/rack-workspaces",
                "executor": {"image": "docker.io/library/rust:bookworm"},
                "repositories": []
            }"#,
        )
        .unwrap();
        assert_eq!(document.workspace_root, "/srv/rack-workspaces");
        assert!(document.repositories.is_empty());
    }
}
