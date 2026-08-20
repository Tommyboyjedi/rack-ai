#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Blocked,
}

impl RunStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::TimedOut | Self::Blocked
        )
    }
}

#[cfg(test)]
mod tests {
    use super::RunStatus;

    #[test]
    fn identifies_terminal_states() {
        assert!(RunStatus::Succeeded.is_terminal());
        assert!(RunStatus::Failed.is_terminal());
        assert!(RunStatus::TimedOut.is_terminal());
        assert!(RunStatus::Blocked.is_terminal());
    }

    #[test]
    fn identifies_non_terminal_states() {
        assert!(!RunStatus::Queued.is_terminal());
        assert!(!RunStatus::Running.is_terminal());
    }
}
