use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandEvidence {
    argv: Vec<String>,
    exit_code: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completed: Option<u64>,
}

impl CommandEvidence {
    pub fn new(argv: Vec<String>, exit_code: i32) -> Self {
        Self {
            argv,
            exit_code,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            started: None,
            completed: None,
        }
    }

    pub fn with_stdout(mut self, stdout: String) -> Self {
        self.stdout = stdout;
        self
    }

    pub fn with_stderr(mut self, stderr: String) -> Self {
        self.stderr = stderr;
        self
    }

    pub fn with_timed_out(mut self, timed_out: bool) -> Self {
        self.timed_out = timed_out;
        self
    }

    pub fn with_started(mut self, started: u64) -> Self {
        self.started = Some(started);
        self
    }

    pub fn with_completed(mut self, completed: u64) -> Self {
        self.completed = Some(completed);
        self
    }

    pub fn with_timing(mut self, started: u64, completed: u64) -> Self {
        self.started = Some(started);
        self.completed = Some(completed);
        self
    }

    pub fn argv(&self) -> &[String] {
        self.argv.as_slice()
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn stdout(&self) -> &str {
        self.stdout.as_str()
    }

    pub fn stderr(&self) -> &str {
        self.stderr.as_str()
    }

    pub fn timed_out(&self) -> bool {
        self.timed_out
    }

    pub fn started(&self) -> Option<u64> {
        self.started
    }

    pub fn completed(&self) -> Option<u64> {
        self.completed
    }

    pub fn duration_seconds(&self) -> Option<u64> {
        self.started
            .zip(self.completed)
            .map(|(started, completed)| completed.saturating_sub(started))
    }

    pub fn succeeded(&self) -> bool {
        self.exit_code == 0 && !self.timed_out
    }
}

#[cfg(test)]
mod tests {
    use super::CommandEvidence;

    #[test]
    fn treats_nonzero_or_timeout_as_failure() {
        let failed = CommandEvidence::new(vec!["cargo".to_string(), "test".to_string()], 1);
        let timed_out = CommandEvidence::new(vec!["cargo".to_string()], 124).with_timed_out(true);
        assert!(!failed.succeeded());
        assert!(!timed_out.succeeded());
    }
}
