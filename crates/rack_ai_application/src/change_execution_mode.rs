#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeExecutionMode {
    PrepareOnly,
    ChecksOnly,
    ImplementAndVerify,
}

impl ChangeExecutionMode {
    pub fn runs_implementer(self) -> bool {
        matches!(self, Self::ImplementAndVerify)
    }

    pub fn runs_checks(self) -> bool {
        matches!(self, Self::ChecksOnly | Self::ImplementAndVerify)
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeExecutionMode;

    #[test]
    fn implement_mode_runs_coder_and_checks() {
        assert!(ChangeExecutionMode::ImplementAndVerify.runs_implementer());
        assert!(ChangeExecutionMode::ImplementAndVerify.runs_checks());
        assert!(!ChangeExecutionMode::PrepareOnly.runs_implementer());
        assert!(ChangeExecutionMode::ChecksOnly.runs_checks());
    }
}
