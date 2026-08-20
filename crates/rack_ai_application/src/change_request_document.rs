use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeRequestDocument {
    pub change_id: String,
    pub repository: ChangeRepositoryDocument,
    pub task: String,
    pub allowed_paths: Vec<String>,
    pub acceptance: AcceptanceDocument,
    pub limits: LimitsDocument,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeRepositoryDocument {
    pub id: String,
    #[serde(default)]
    pub registered_root: Option<String>,
    pub base_ref: String,
    #[serde(default)]
    pub base_sha: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcceptanceDocument {
    pub commands: Vec<Vec<String>>,
    #[serde(default)]
    pub required_artifacts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LimitsDocument {
    pub max_implementation_attempts: u32,
    pub timeout_seconds: u32,
    #[serde(default = "disabled_network")]
    pub network: String,
}

fn disabled_network() -> String {
    "disabled".to_string()
}

#[cfg(test)]
mod tests {
    use super::ChangeRequestDocument;

    #[test]
    fn parses_example_document() {
        let json = r#"{
            "change_id": "adaptos-001",
            "repository": {"id": "adaptos", "base_ref": "main"},
            "task": "Add a feature.",
            "allowed_paths": ["src/", "Cargo.toml"],
            "acceptance": {"commands": [["cargo", "test"]]},
            "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
        }"#;
        let document = serde_json::from_str::<ChangeRequestDocument>(json).unwrap();
        assert_eq!(document.change_id, "adaptos-001");
        assert_eq!(document.limits.network, "disabled");
        assert!(document.repository.base_sha.is_none());
    }
}
