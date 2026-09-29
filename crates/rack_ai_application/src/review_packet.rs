use serde::Deserialize;
use serde::Serialize;

use rack_ai_domain::AcceptanceVerdict;
use rack_ai_domain::ChangeStatus;
use rack_ai_domain::RetentionStatus;

use crate::ChangeRequest;
use crate::ChangeWorkspace;
use crate::CommandEvidence;
use crate::GenericWorkerSelectionDecision;
use crate::GitEvidence;
use crate::ToolCallRecord;
use crate::WorkerExecutionProvenance;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExecutionActivityEvent {
    phase: String,
    outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl ExecutionActivityEvent {
    pub fn new(phase: impl Into<String>, outcome: impl Into<String>) -> Self {
        Self {
            phase: phase.into(),
            outcome: outcome.into(),
            started: None,
            completed: None,
            detail: None,
        }
    }

    pub fn with_timing(mut self, started: u64, completed: u64) -> Self {
        self.started = Some(started);
        self.completed = Some(completed);
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn phase(&self) -> &str {
        self.phase.as_str()
    }

    pub fn outcome(&self) -> &str {
        self.outcome.as_str()
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

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

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
    #[serde(default)]
    worker_provenance: Option<WorkerExecutionProvenance>,
    #[serde(default)]
    selection_decision: Option<GenericWorkerSelectionDecision>,
    #[serde(default)]
    activity_events: Vec<ExecutionActivityEvent>,
    #[serde(default)]
    tool_calls: Vec<ToolCallRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution_budget_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deadline_ended_attempt: Option<String>,
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
            worker_provenance: None,
            selection_decision: None,
            activity_events: Vec::new(),
            tool_calls: Vec::new(),
            execution_budget_seconds: None,
            deadline_ended_attempt: None,
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
            worker_provenance: None,
            selection_decision: None,
            activity_events: Vec::new(),
            tool_calls: Vec::new(),
            execution_budget_seconds: None,
            deadline_ended_attempt: None,
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

    pub fn with_head_sha(mut self, head_sha: String) -> Self {
        self.head_sha = head_sha;
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

    pub fn with_worker_provenance(mut self, provenance: WorkerExecutionProvenance) -> Self {
        self.worker_provenance = Some(provenance);
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

    pub fn head_sha(&self) -> &str {
        self.head_sha.as_str()
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

    pub fn with_selection_decision(mut self, decision: GenericWorkerSelectionDecision) -> Self {
        self.selection_decision = Some(decision);
        self
    }

    pub fn selection_decision(&self) -> Option<&GenericWorkerSelectionDecision> {
        self.selection_decision.as_ref()
    }

    pub fn worker_provenance(&self) -> Option<&WorkerExecutionProvenance> {
        self.worker_provenance.as_ref()
    }

    pub fn with_activity_event(mut self, event: ExecutionActivityEvent) -> Self {
        self.activity_events.push(event);
        self
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCallRecord>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    pub fn with_execution_budget_seconds(mut self, seconds: u64) -> Self {
        self.execution_budget_seconds = Some(seconds);
        self
    }

    pub fn with_deadline_ended_attempt(mut self, phase: impl Into<String>) -> Self {
        self.deadline_ended_attempt = Some(phase.into());
        self
    }

    pub fn activity_events(&self) -> &[ExecutionActivityEvent] {
        self.activity_events.as_slice()
    }

    pub fn tool_calls(&self) -> &[ToolCallRecord] {
        self.tool_calls.as_slice()
    }

    pub fn execution_budget_seconds(&self) -> Option<u64> {
        self.execution_budget_seconds
    }

    pub fn deadline_ended_attempt(&self) -> Option<&str> {
        self.deadline_ended_attempt.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::ReviewPacket;
    use crate::WorkerExecutionProvenance;

    fn provenance() -> WorkerExecutionProvenance {
        WorkerExecutionProvenance {
            worker_id: "local-coder".to_string(),
            worker_role: "implementer-tester".to_string(),
            worker_kind: "jcode".to_string(),
            model_id: "eqaq-v2-local-coder".to_string(),
            provider_profile: "local-coder".to_string(),
            resource_id: "gpu-2060".to_string(),
            backend: "jcode".to_string(),
            tool_profile: Some("minimal".to_string()),
        }
    }

    #[test]
    fn reload_preserves_worker_provenance_without_endpoint_or_secret_data() {
        let packet = ReviewPacket::new("job-1".to_string(), "fixture".to_string())
            .with_worker_provenance(provenance());
        let json = serde_json::to_string(&packet).unwrap();
        let reloaded: ReviewPacket = serde_json::from_str(&json).unwrap();

        assert_eq!(reloaded.worker_provenance(), packet.worker_provenance());
        assert!(!json.contains("endpoint"));
        assert!(!json.contains("token"));
    }

    #[test]
    fn old_packet_without_worker_provenance_remains_readable() {
        let packet = ReviewPacket::new("job-1".to_string(), "fixture".to_string());
        let mut old = serde_json::to_value(packet).unwrap();
        old.as_object_mut().unwrap().remove("worker_provenance");

        assert_eq!(
            serde_json::from_value::<ReviewPacket>(old)
                .unwrap()
                .worker_provenance(),
            None
        );
    }
}
