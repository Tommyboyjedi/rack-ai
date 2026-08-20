use crate::CommandEvidence;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceExecutionResult {
    evidence: CommandEvidence,
    content: String,
}

impl WorkspaceExecutionResult {
    pub fn new(evidence: CommandEvidence) -> Self {
        Self {
            content: String::new(),
            evidence,
        }
    }

    pub fn with_content(mut self, content: String) -> Self {
        self.content = content;
        self
    }

    pub fn evidence(&self) -> &CommandEvidence {
        &self.evidence
    }

    pub fn content(&self) -> &str {
        self.content.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceExecutionResult;
    use crate::CommandEvidence;

    #[test]
    fn carries_command_evidence() {
        let result = WorkspaceExecutionResult::new(CommandEvidence::new(
            vec!["cat".to_string(), "file".to_string()],
            0,
        ))
        .with_content("hello".to_string());
        assert_eq!(result.content(), "hello");
        assert!(result.evidence().succeeded());
    }
}
