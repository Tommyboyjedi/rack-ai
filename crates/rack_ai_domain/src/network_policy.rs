use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicy {
    Disabled,
}

impl NetworkPolicy {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "disabled" => Ok(Self::Disabled),
            _ => Err("external-repository changes require network=disabled".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NetworkPolicy;

    #[test]
    fn accepts_disabled() {
        assert_eq!(
            NetworkPolicy::parse("disabled").unwrap(),
            NetworkPolicy::Disabled
        );
    }

    #[test]
    fn rejects_enabled_for_v1() {
        assert_eq!(
            NetworkPolicy::parse("enabled"),
            Err("external-repository changes require network=disabled".to_string())
        );
    }
}
