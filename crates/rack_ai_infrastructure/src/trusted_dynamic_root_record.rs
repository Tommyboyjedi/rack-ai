use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TrustedDynamicRootRecord {
    pub id: String,
    pub root: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::TrustedDynamicRootRecord;

    #[test]
    fn defaults_enabled() {
        let record = serde_json::from_str::<TrustedDynamicRootRecord>(
            "{\"id\":\"athba-projects\",\"root\":\"/srv/ATHBA/state/projects\"}",
        )
        .unwrap();
        assert!(record.enabled);
    }
}
