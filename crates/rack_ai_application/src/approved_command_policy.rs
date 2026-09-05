use rack_ai_domain::AcceptanceCommand;

use crate::CommandPolicy;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedCommandPolicy {
    legacy_programs: Vec<String>,
}

impl ApprovedCommandPolicy {
    pub fn new(programs: Vec<String>) -> Result<Self, String> {
        Ok(Self {
            legacy_programs: programs,
        })
    }
}

impl Default for ApprovedCommandPolicy {
    fn default() -> Self {
        Self::new(Vec::new()).expect("generic command policy must construct")
    }
}

impl CommandPolicy for ApprovedCommandPolicy {
    fn assert_allowed(&self, command: &AcceptanceCommand) -> Result<(), String> {
        let _ = &self.legacy_programs;
        if shell_name(command.program()) {
            return Err(format!(
                "acceptance command {} must not invoke a shell interpreter",
                command.program()
            ));
        }
        Ok(())
    }
}

fn shell_name(program: &str) -> bool {
    let normalized = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "cmd"
            | "cmd.exe"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
    )
}

#[cfg(test)]
mod tests {
    use super::ApprovedCommandPolicy;
    use crate::CommandPolicy;
    use rack_ai_domain::AcceptanceCommand;

    #[test]
    fn allows_direct_executables_and_rejects_shells() {
        let policy = ApprovedCommandPolicy::default();
        let cargo = AcceptanceCommand::new(vec!["cargo".to_string(), "test".to_string()]).unwrap();
        let absolute = AcceptanceCommand::new(vec![
            "/srv/ATHBA/.venv/bin/python".to_string(),
            "scripts/assert_test_fails.py".to_string(),
        ])
        .unwrap();
        let local = AcceptanceCommand::new(vec!["./tools/test-runner".to_string()]).unwrap();
        let shell = AcceptanceCommand::new(vec!["bash".to_string()]).unwrap();
        let absolute_shell = AcceptanceCommand::new(vec!["/bin/bash".to_string()]).unwrap();
        assert!(policy.assert_allowed(&cargo).is_ok());
        assert!(policy.assert_allowed(&absolute).is_ok());
        assert!(policy.assert_allowed(&local).is_ok());
        assert_eq!(
            policy.assert_allowed(&shell),
            Err("acceptance command bash must not invoke a shell interpreter".to_string())
        );
        assert_eq!(
            policy.assert_allowed(&absolute_shell),
            Err("acceptance command /bin/bash must not invoke a shell interpreter".to_string())
        );
    }

    #[test]
    fn legacy_program_list_is_not_required() {
        let policy = ApprovedCommandPolicy::new(vec!["cargo".to_string()]).unwrap();
        let python =
            AcceptanceCommand::new(vec!["/srv/ATHBA/.venv/bin/python".to_string()]).unwrap();
        assert!(policy.assert_allowed(&python).is_ok());
    }
}
