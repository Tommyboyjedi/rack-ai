use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositoryRecord {
    pub id: String,
    pub root: String,
    #[serde(default = "default_base_ref")]
    pub default_base_ref: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_base_ref() -> String {
    "main".to_string()
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::RepositoryRecord;

    #[test]
    fn defaults_enabled_main() {
        let record = serde_json::from_str::<RepositoryRecord>(
            "{\"id\":\"adaptos\",\"root\":\"/srv/projects/adaptos\"}",
        )
        .unwrap();
        assert_eq!(record.default_base_ref, "main");
        assert!(record.enabled);
    }
}
