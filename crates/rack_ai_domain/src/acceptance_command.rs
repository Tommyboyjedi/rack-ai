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
        if program.contains('/') || program.contains('\\') || program.contains("..") {
            return Err("acceptance command must use an approved program name".to_string());
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

#[cfg(test)]
mod tests {
    use super::AcceptanceCommand;

    #[test]
    fn rejects_empty_or_path_program() {
        assert!(AcceptanceCommand::new(vec![]).is_err());
        assert!(AcceptanceCommand::new(vec!["/bin/sh".to_string()]).is_err());
        assert!(AcceptanceCommand::new(vec!["../cargo".to_string()]).is_err());
    }

    #[test]
    fn keeps_argv() {
        let command =
            AcceptanceCommand::new(vec!["cargo".to_string(), "test".to_string()]).unwrap();
        assert_eq!(command.program(), "cargo");
        assert_eq!(command.argv(), ["cargo", "test"]);
    }
}
