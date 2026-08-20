use rack_ai_domain::AcceptanceCommand;
use rack_ai_domain::RequiredArtifact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptancePolicy {
    commands: Vec<AcceptanceCommand>,
    required_artifacts: Vec<RequiredArtifact>,
}

impl AcceptancePolicy {
    pub fn new(commands: Vec<AcceptanceCommand>) -> Result<Self, String> {
        if commands.is_empty() {
            return Err("acceptance commands cannot be empty".to_string());
        }
        Ok(Self {
            commands,
            required_artifacts: Vec::new(),
        })
    }

    pub fn with_required_artifacts(mut self, required_artifacts: Vec<RequiredArtifact>) -> Self {
        self.required_artifacts = required_artifacts;
        self
    }

    pub fn commands(&self) -> &[AcceptanceCommand] {
        self.commands.as_slice()
    }

    pub fn required_artifacts(&self) -> &[RequiredArtifact] {
        self.required_artifacts.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::AcceptancePolicy;
    use rack_ai_domain::AcceptanceCommand;

    #[test]
    fn rejects_empty_commands() {
        assert_eq!(
            AcceptancePolicy::new(vec![]),
            Err("acceptance commands cannot be empty".to_string())
        );
    }

    #[test]
    fn keeps_commands() {
        let policy = AcceptancePolicy::new(vec![
            AcceptanceCommand::new(vec!["cargo".to_string(), "test".to_string()]).unwrap(),
        ])
        .unwrap();
        assert_eq!(policy.commands()[0].program(), "cargo");
    }
}
