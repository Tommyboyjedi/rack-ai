use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TrustedEnvironmentRootRecord {
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
    use super::TrustedEnvironmentRootRecord;

    #[test]
    fn defaults_enabled() {
        let record = serde_json::from_str::<TrustedEnvironmentRootRecord>(
            r#"{"id":"runtime-0","root":"/srv/environments"}"#,
        )
        .unwrap();
        assert!(record.enabled);
    }
}
