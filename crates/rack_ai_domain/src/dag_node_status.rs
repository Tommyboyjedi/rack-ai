use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DagNodeStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    TimedOut,
}

impl DagNodeStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::TimedOut)
    }
}

#[cfg(test)]
mod tests {
    use super::DagNodeStatus;

    #[test]
    fn identifies_terminal_statuses() {
        assert!(DagNodeStatus::Succeeded.is_terminal());
        assert!(DagNodeStatus::Failed.is_terminal());
        assert!(DagNodeStatus::TimedOut.is_terminal());
    }

    #[test]
    fn identifies_non_terminal_statuses() {
        assert!(!DagNodeStatus::Pending.is_terminal());
        assert!(!DagNodeStatus::Running.is_terminal());
    }
}
