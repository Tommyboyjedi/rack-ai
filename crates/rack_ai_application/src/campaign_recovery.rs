use serde::Deserialize;
use serde::Serialize;

use crate::FailureClassification;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDecisionKind {
    Repair,
    Replan,
    BlockInsufficientAuthority,
    BlockTerminal,
    RetryTransient,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryFailureKind {
    LocalImplementationDefect,
    CompatibilityConstraint,
    StrategyFailure,
    RepeatedFailure,
    InsufficientAuthority,
    TransientFailure,
    TerminalCondition,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryWorkerAction {
    SameWorker,
    FallbackWorker,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryCommandFailure {
    pub command: String,
    pub exit_code: i32,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryToolAttempt {
    pub name: String,
    pub target_path: Option<String>,
    pub allowed: Option<bool>,
    pub result_excerpt: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryAttemptSummary {
    pub attempt: usize,
    pub worker_id: String,
    pub attempt_kind: String,
    pub classification: Option<FailureClassification>,
    pub rationale: String,
    pub launch_instruction: Option<String>,
    pub next_instruction: Option<String>,
    pub recovery_decision: Option<RecoveryDecisionKind>,
    pub fingerprint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryContext {
    pub campaign_id: String,
    pub step_id: String,
    pub original_task: String,
    pub campaign_permitted_paths: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub required_changed_paths: Vec<String>,
    pub acceptance_commands: Vec<Vec<String>>,
    pub changed_paths: Vec<String>,
    pub git_status: String,
    pub diff_stat: String,
    pub diff_excerpt: String,
    pub failure_classification: FailureClassification,
    pub failure_rationale: String,
    pub command_failure: Option<RecoveryCommandFailure>,
    pub tool_attempts: Vec<RecoveryToolAttempt>,
    pub previous_attempts: Vec<RecoveryAttemptSummary>,
    pub repeated_failure_count: usize,
    pub current_fingerprint: String,
    pub remaining_attempt_budget: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryDecision {
    pub kind: RecoveryDecisionKind,
    pub failure_kind: RecoveryFailureKind,
    pub rationale: String,
    pub evidence_refs: Vec<String>,
    pub constraint_conflict: bool,
    pub same_strategy_viable: bool,
    pub worker_action: RecoveryWorkerAction,
    pub next_instruction: Option<String>,
    pub insufficient_authority: bool,
    pub stagnation_detected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryReasoningRequest {
    context: RecoveryContext,
    timeout_seconds: u32,
}

impl RecoveryReasoningRequest {
    pub fn new(context: RecoveryContext, timeout_seconds: u32) -> Self {
        Self {
            context,
            timeout_seconds: timeout_seconds.max(1),
        }
    }

    pub fn context(&self) -> &RecoveryContext {
        &self.context
    }

    pub fn timeout_seconds(&self) -> u32 {
        self.timeout_seconds
    }

    pub fn prompt(&self) -> String {
        let context_json = serde_json::to_string_pretty(&self.context)
            .unwrap_or_else(|_| "{\"error\":\"failed to render recovery context\"}".to_string());
        format!(
            "You are Rack AI's read-only recovery coordinator. Diagnose a failed implementation attempt and choose the safest bounded next action.\n\
Do not write files, do not call tools, do not broaden authority, and do not invent new campaign steps.\n\
A caller or file outside allowed_paths may be a compatibility constraint that can be READ but not modified.\n\
Choose exactly one JSON object with this schema:\n\
{{\n  \"kind\":\"repair|replan|block_insufficient_authority|block_terminal|retry_transient\",\n  \"failure_kind\":\"local_implementation_defect|compatibility_constraint|strategy_failure|repeated_failure|insufficient_authority|transient_failure|terminal_condition\",\n  \"rationale\":\"concise operational diagnosis\",\n  \"evidence_refs\":[\"git-evidence.json\",\"command-evidence.json\"],\n  \"constraint_conflict\":true|false,\n  \"same_strategy_viable\":true|false,\n  \"worker_action\":\"same_worker|fallback_worker\",\n  \"next_instruction\":null|\"bounded next implementation instruction\",\n  \"insufficient_authority\":true|false,\n  \"stagnation_detected\":true|false\n}}\n\
Use repair only when the current implementation strategy is still valid.\n\
Use replan when the worker must preserve compatibility or materially change strategy within the same authority.\n\
Use block_insufficient_authority when the needed change is outside allowed_paths/permitted_paths.\n\
Use retry_transient only for clearly transient execution or transport problems.\n\
Recovery context:\n{}",
            context_json
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryReasoningResult {
    pub decision: RecoveryDecision,
    pub prompt: String,
    pub raw_output: String,
}

pub trait RecoveryReasoner {
    fn diagnose(
        &self,
        request: &RecoveryReasoningRequest,
    ) -> Result<RecoveryReasoningResult, String>;
}

pub fn parse_recovery_output(raw: &str) -> Result<RecoveryDecision, String> {
    let json = extract_json_object(raw)
        .ok_or_else(|| "recovery diagnosis did not return a JSON object".to_string())?;
    serde_json::from_str::<RecoveryDecision>(&json).map_err(|error| error.to_string())
}

fn extract_json_object(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end >= start).then(|| raw[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::RecoveryDecisionKind;
    use super::RecoveryFailureKind;
    use super::RecoveryWorkerAction;
    use super::parse_recovery_output;

    #[test]
    fn parses_fenced_recovery_json() {
        let raw = "```json\n{\"kind\":\"replan\",\"failure_kind\":\"compatibility_constraint\",\"rationale\":\"caller is immutable\",\"evidence_refs\":[\"git-evidence.json\"],\"constraint_conflict\":true,\"same_strategy_viable\":false,\"worker_action\":\"fallback_worker\",\"next_instruction\":\"Preserve src/main.rs and revise service.rs only.\",\"insufficient_authority\":false,\"stagnation_detected\":true}\n```";
        let decision = parse_recovery_output(raw).unwrap();
        assert_eq!(decision.kind, RecoveryDecisionKind::Replan);
        assert_eq!(
            decision.failure_kind,
            RecoveryFailureKind::CompatibilityConstraint
        );
        assert_eq!(decision.worker_action, RecoveryWorkerAction::FallbackWorker);
        assert!(decision.stagnation_detected);
    }
}
