use rack_ai_domain::AcceptanceCommand;

use crate::CommandPolicy;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedCommandPolicy {
    programs: Vec<String>,
}

impl ApprovedCommandPolicy {
    pub fn new(programs: Vec<String>) -> Result<Self, String> {
        if programs.is_empty() {
            return Err("approved command policy cannot be empty".to_string());
        }
        Ok(Self { programs })
    }

    pub fn default_programs() -> Vec<String> {
        [
            "cargo", "rustc", "rustfmt", "python3", "pytest", "npm", "node", "go", "make",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }
}

impl Default for ApprovedCommandPolicy {
    fn default() -> Self {
        Self {
            programs: Self::default_programs(),
        }
    }
}

impl CommandPolicy for ApprovedCommandPolicy {
    fn assert_allowed(&self, command: &AcceptanceCommand) -> Result<(), String> {
        if self
            .programs
            .iter()
            .any(|program| program == command.program())
        {
            return Ok(());
        }
        Err(format!(
            "acceptance command {} is not approved",
            command.program()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::ApprovedCommandPolicy;
    use crate::CommandPolicy;
    use rack_ai_domain::AcceptanceCommand;

    #[test]
    fn allows_cargo_and_rejects_shell() {
        let policy = ApprovedCommandPolicy::default();
        let cargo = AcceptanceCommand::new(vec!["cargo".to_string(), "test".to_string()]).unwrap();
        let shell = AcceptanceCommand::new(vec!["bash".to_string()]).unwrap();
        assert!(policy.assert_allowed(&cargo).is_ok());
        assert_eq!(
            policy.assert_allowed(&shell),
            Err("acceptance command bash is not approved".to_string())
        );
    }
}
