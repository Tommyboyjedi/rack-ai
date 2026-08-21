use serde::Deserialize;
use serde::Serialize;

use rack_ai_domain::AcceptanceVerdict;
use rack_ai_domain::ChangeStatus;
use rack_ai_domain::RetentionStatus;

use crate::ChangeRequest;
use crate::ChangeWorkspace;
use crate::CommandEvidence;
use crate::GitEvidence;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewPacket {
    change_id: String,
    repository_id: String,
    registered_root: String,
    base_ref: String,
    base_sha: String,
    branch: String,
    worktree_path: String,
    task: String,
    allowed_paths: Vec<String>,
    changed_paths: Vec<String>,
    git_status: String,
    diff_stat: String,
    diff: String,
    head_sha: String,
    commands: Vec<CommandEvidence>,
    required_artifacts: Vec<String>,
    implementer_output: Option<String>,
    acceptance_verdict: Option<AcceptanceVerdict>,
    status: ChangeStatus,
    retention: RetentionStatus,
    last_error: Option<String>,
}

impl ReviewPacket {
    pub fn new(change_id: String, repository_id: String) -> Self {
        Self {
            change_id,
            repository_id,
            registered_root: String::new(),
            base_ref: String::new(),
            base_sha: String::new(),
            branch: String::new(),
            worktree_path: String::new(),
            task: String::new(),
            allowed_paths: Vec::new(),
            changed_paths: Vec::new(),
            git_status: String::new(),
            diff_stat: String::new(),
            diff: String::new(),
            head_sha: String::new(),
            commands: Vec::new(),
            required_artifacts: Vec::new(),
            implementer_output: None,
            acceptance_verdict: None,
            status: ChangeStatus::Prepared,
            retention: RetentionStatus::Retained,
            last_error: None,
        }
    }

    pub fn from_request(request: &ChangeRequest) -> Self {
        Self {
            change_id: request.change_id().value().to_string(),
            repository_id: request.repository().id().value().to_string(),
            registered_root: request.repository().registered_root().display().to_string(),
            base_ref: request.repository().base_ref().value().to_string(),
            base_sha: request.repository().base_sha().value().to_string(),
            branch: String::new(),
            worktree_path: String::new(),
            task: request.task().value().to_string(),
            allowed_paths: request
                .allowed_paths()
                .values()
                .iter()
                .map(|path| path.value().to_string())
                .collect(),
            changed_paths: Vec::new(),
            git_status: String::new(),
            diff_stat: String::new(),
            diff: String::new(),
            head_sha: String::new(),
            commands: Vec::new(),
            required_artifacts: request
                .acceptance()
                .required_artifacts()
                .iter()
                .map(|item| item.value().to_string())
                .collect(),
            implementer_output: None,
            acceptance_verdict: None,
            status: ChangeStatus::Prepared,
            retention: RetentionStatus::Retained,
            last_error: None,
        }
    }

    pub fn with_workspace(mut self, workspace: &ChangeWorkspace) -> Self {
        self.branch = workspace.branch_name().to_string();
        self.worktree_path = workspace.worktree_path().display().to_string();
        self
    }

    pub fn with_git_evidence(mut self, evidence: &GitEvidence) -> Self {
        self.changed_paths = evidence.changed_paths().to_vec();
        self.git_status = evidence.status().to_string();
        self.diff_stat = evidence.diff_stat().to_string();
        self.diff = evidence.diff().to_string();
        self.head_sha = evidence.head_sha().value().to_string();
        self
    }

    pub fn with_commands(mut self, commands: Vec<CommandEvidence>) -> Self {
        self.commands = commands;
        self
    }

    pub fn with_status(mut self, status: ChangeStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_last_error(mut self, last_error: Option<String>) -> Self {
        self.last_error = last_error;
        self
    }

    pub fn with_implementer_output(mut self, implementer_output: String) -> Self {
        self.implementer_output = Some(implementer_output);
        self
    }

    pub fn with_acceptance_verdict(mut self, verdict: AcceptanceVerdict) -> Self {
        self.acceptance_verdict = Some(verdict);
        self
    }

    pub fn change_id(&self) -> &str {
        self.change_id.as_str()
    }

    pub fn worktree_path(&self) -> &str {
        self.worktree_path.as_str()
    }

    pub fn branch(&self) -> &str {
        self.branch.as_str()
    }

    pub fn base_sha(&self) -> &str {
        self.base_sha.as_str()
    }

    pub fn status(&self) -> &ChangeStatus {
        &self.status
    }

    pub fn changed_paths(&self) -> &[String] {
        self.changed_paths.as_slice()
    }

    pub fn last_error(&self) -> Option<&String> {
        self.last_error.as_ref()
    }

    pub fn commands(&self) -> &[CommandEvidence] {
        self.commands.as_slice()
    }

    pub fn implementer_output(&self) -> Option<&String> {
        self.implementer_output.as_ref()
    }

    pub fn acceptance_verdict(&self) -> Option<&AcceptanceVerdict> {
        self.acceptance_verdict.as_ref()
    }
}
