use serde::Deserialize;
use serde::Serialize;

/// Result of deterministic Git/path/acceptance gates.
/// This is not a local-primary model verifier verdict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceVerdict {
    Approved,
    Rejected,
}

impl AcceptanceVerdict {
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

#[cfg(test)]
mod tests {
    use super::AcceptanceVerdict;

    #[test]
    fn distinguishes_approval() {
        assert!(AcceptanceVerdict::Approved.is_approved());
        assert!(!AcceptanceVerdict::Rejected.is_approved());
    }
}
