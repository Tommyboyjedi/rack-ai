use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcceptanceCommand(Vec<String>);

impl AcceptanceCommand {
    pub fn new(argv: Vec<String>) -> Result<Self, String> {
        if argv.is_empty() || argv.iter().any(|item| item.is_empty()) {
            return Err("acceptance command cannot be empty".to_string());
        }
        let program = &argv[0];
        if contains_parent_traversal(program) {
            return Err("acceptance command executable must not use parent traversal".to_string());
        }
        Ok(Self(argv))
    }

    pub fn program(&self) -> &str {
        self.0[0].as_str()
    }

    pub fn argv(&self) -> &[String] {
        self.0.as_slice()
    }
}

fn contains_parent_traversal(program: &str) -> bool {
    program
        .split(['/', '\\'])
        .any(|component| component == "..")
}

#[cfg(test)]
mod tests {
    use super::AcceptanceCommand;

    #[test]
    fn rejects_empty_or_parent_traversal_program() {
        assert!(AcceptanceCommand::new(vec![]).is_err());
        assert!(AcceptanceCommand::new(vec!["../cargo".to_string()]).is_err());
        assert!(AcceptanceCommand::new(vec!["bin/../cargo".to_string()]).is_err());
    }

    #[test]
    fn keeps_argv() {
        let command =
            AcceptanceCommand::new(vec!["cargo".to_string(), "test".to_string()]).unwrap();
        assert_eq!(command.program(), "cargo");
        assert_eq!(command.argv(), ["cargo", "test"]);
    }

    #[test]
    fn accepts_absolute_and_workspace_local_executable_paths() {
        let absolute = AcceptanceCommand::new(vec![
            "/srv/ATHBA/.venv/bin/python".to_string(),
            "scripts/assert_test_fails.py".to_string(),
        ])
        .unwrap();
        let local = AcceptanceCommand::new(vec![
            "./.venv/bin/python".to_string(),
            "-m".to_string(),
            "pytest".to_string(),
        ])
        .unwrap();
        assert_eq!(absolute.program(), "/srv/ATHBA/.venv/bin/python");
        assert_eq!(local.program(), "./.venv/bin/python");
    }
}
