use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierVerdict {
    Approved,
    Rejected,
}

impl VerifierVerdict {
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

#[cfg(test)]
mod tests {
    use super::VerifierVerdict;

    #[test]
    fn distinguishes_approval() {
        assert!(VerifierVerdict::Approved.is_approved());
        assert!(!VerifierVerdict::Rejected.is_approved());
    }
}
