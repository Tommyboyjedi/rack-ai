use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Prepared,
    ChecksPassed,
    ChecksFailed,
    PathPolicyFailed,
    ExecutorUnavailable,
    Failed,
}

impl ChangeStatus {
    pub fn is_successful(&self) -> bool {
        matches!(self, Self::Prepared | Self::ChecksPassed)
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeStatus;

    #[test]
    fn identifies_successful_prepare_and_checks() {
        assert!(ChangeStatus::Prepared.is_successful());
        assert!(ChangeStatus::ChecksPassed.is_successful());
        assert!(!ChangeStatus::Failed.is_successful());
        assert!(!ChangeStatus::ExecutorUnavailable.is_successful());
    }
}
