use crate::CampaignStep;
use crate::CampaignStepKind;
use crate::CommandEvidence;
use crate::CoordinatorReview;
use crate::CoordinatorReviewDisposition;
use crate::FailureClassification;
use crate::GitEvidence;
use crate::campaign_paths::assert_authorized_paths;
use crate::campaign_paths::required_prefix_satisfied;
use crate::source_paths;

pub struct ReviewInput<'a> {
    pub step: &'a CampaignStep,
    pub evidence: &'a GitEvidence,
    pub commands_succeeded: bool,
    pub missing_artifacts: Vec<String>,
    pub implementer_output: Option<&'a str>,
    pub protocol_error: Option<&'a str>,
    pub worker_error: Option<&'a str>,
    pub tool_calls: usize,
    pub used_host_shell: bool,
}

pub fn looks_like_markdown_tool_call(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("<tool_call>") || lower.contains("</tool_call>") {
        return true;
    }
    let fenced = text.contains("```");
    let mentions_tool = lower.contains("tool_call")
        || lower.contains("\"name\": \"write\"")
        || lower.contains("\"name\":\"write\"")
        || lower.contains("\"name\": \"bash\"")
        || lower.contains("invoke write")
        || (lower.contains("file_path") && lower.contains("content"));
    fenced && mentions_tool
}

const ACCEPTANCE_EVIDENCE_MAX_LINES: usize = 12;
const ACCEPTANCE_EVIDENCE_MAX_CHARS: usize = 600;

pub fn repair_instruction(
    step: &CampaignStep,
    review: &CoordinatorReview,
    diff_summary: &str,
    commands: &[CommandEvidence],
) -> String {
    let acceptance_context = bounded_acceptance_context(review, commands);
    let acceptance_block = acceptance_context
        .as_deref()
        .map(|value| format!("\nAcceptance failure evidence:\n{value}"))
        .unwrap_or_default();
    format!(
        "Repair the previous attempt for step {}.\nOriginal task: {}\nRejection classification: {}\nRejection rationale: {}\nRequired changed paths: {}\nAllowed paths: {}\nDiff summary: {}{}\nDo not add new tasks. Do not broaden allowed_paths, acceptance commands, duration, resource limits, or promotion authority.",
        step.id,
        step.task,
        review
            .classification
            .map(|value| value.as_str().to_string())
            .unwrap_or_else(|| "rejected".to_string()),
        review.rationale,
        step.required_changed_paths.join(", "),
        step.allowed_paths.join(", "),
        diff_summary,
        acceptance_block,
    )
}

fn bounded_acceptance_context(
    review: &CoordinatorReview,
    commands: &[CommandEvidence],
) -> Option<String> {
    if review.classification != Some(FailureClassification::AcceptanceFailed) {
        return None;
    }
    let failed = commands.iter().find(|command| !command.succeeded())?;
    let mut lines = vec![
        format!("Failing command: {}", failed.argv().join(" ")),
        format!("Exit code: {}", failed.exit_code()),
    ];
    let stderr = bounded_excerpt(failed.stderr());
    if !stderr.is_empty() {
        lines.push(format!("stderr:\n{stderr}"));
    }
    let stdout = bounded_excerpt(failed.stdout());
    if !stdout.is_empty() {
        lines.push(format!("stdout:\n{stdout}"));
    }
    Some(lines.join("\n"))
}

fn bounded_excerpt(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    let mut used = 0usize;
    let mut truncated = false;
    for (index, line) in trimmed.lines().enumerate() {
        if index >= ACCEPTANCE_EVIDENCE_MAX_LINES {
            truncated = true;
            break;
        }
        let separator = usize::from(!lines.is_empty());
        if used + separator + line.len() > ACCEPTANCE_EVIDENCE_MAX_CHARS {
            let remaining = ACCEPTANCE_EVIDENCE_MAX_CHARS.saturating_sub(used + separator);
            if remaining > 0 {
                lines.push(line.chars().take(remaining).collect::<String>());
            }
            truncated = true;
            break;
        }
        used += separator + line.len();
        lines.push(line.to_string());
    }
    let mut excerpt = lines.join("\n");
    if truncated {
        if !excerpt.is_empty() {
            excerpt.push('\n');
        }
        excerpt.push_str("[truncated]");
    }
    excerpt
}

pub fn review_attempt(input: ReviewInput<'_>) -> CoordinatorReview {
    let evidence_refs = vec![
        "git-evidence.json".to_string(),
        "worker-transcript.json".to_string(),
        "command-evidence.json".to_string(),
        "review-packet.json".to_string(),
    ];
    if input.used_host_shell {
        return terminal(
            FailureClassification::ExecutorUnavailable,
            "host-shell or JCode executor is forbidden for campaign work",
            evidence_refs,
        );
    }

    let changed = source_paths(input.evidence.changed_paths());
    if let Err(error) = assert_allowed_paths(input.step, &changed) {
        return terminal(
            FailureClassification::PathPolicyFailed,
            error,
            evidence_refs,
        );
    }

    if let Some(error) = input.worker_error {
        let lower = error.to_lowercase();
        let reviewable_finalization_timeout = lower.contains("timeout")
            && matches!(input.step.kind, CampaignStepKind::Implementation)
            && !changed.is_empty();
        if !reviewable_finalization_timeout {
            if lower.contains("timeout") {
                return retryable(
                    FailureClassification::WorkerTimeout,
                    error.to_string(),
                    evidence_refs,
                );
            }
            if lower.contains("podman") || lower.contains("executor") {
                return terminal(
                    FailureClassification::ExecutorUnavailable,
                    error.to_string(),
                    evidence_refs,
                );
            }
            if lower.contains("model") || lower.contains("endpoint") {
                return retryable(
                    FailureClassification::ModelUnavailable,
                    error.to_string(),
                    evidence_refs,
                );
            }
        }
    }
    if let Some(error) = input.protocol_error {
        return retryable(
            FailureClassification::ToolProtocolViolation,
            format!("tool protocol violation: {error}"),
            evidence_refs,
        );
    }
    if input.tool_calls == 0 {
        if let Some(output) = input.implementer_output {
            if looks_like_markdown_tool_call(output) {
                return retryable(
                    FailureClassification::ToolProtocolViolation,
                    "worker emitted markdown or JSON text instead of a valid tool call",
                    evidence_refs,
                );
            }
        }
    }
    if matches!(input.step.kind, CampaignStepKind::Implementation)
        && input.tool_calls == 0
        && input
            .implementer_output
            .map(|value| value.to_uppercase().contains("COMPLETE"))
            .unwrap_or(false)
        && source_paths(input.evidence.changed_paths()).is_empty()
    {
        // COMPLETE with no tools and no diff is classified below as no_change.
    }

    match input.step.kind {
        CampaignStepKind::Implementation => review_implementation(input, changed, evidence_refs),
        CampaignStepKind::Verification => review_verification(input, evidence_refs),
    }
}

fn review_implementation(
    input: ReviewInput<'_>,
    changed: Vec<String>,
    evidence_refs: Vec<String>,
) -> CoordinatorReview {
    if changed.is_empty() {
        return retryable(
            FailureClassification::NoChange,
            "implementation produced no source diff; passing checks are not an accepted disposition",
            evidence_refs,
        );
    }
    if let Err(error) = assert_required_changed_paths(input.step, &changed) {
        return retryable(FailureClassification::NoChange, error, evidence_refs);
    }
    if !input.missing_artifacts.is_empty() {
        return retryable(
            FailureClassification::ArtifactMissing,
            format!(
                "required artifacts missing: {}",
                input.missing_artifacts.join(", ")
            ),
            evidence_refs,
        );
    }
    if !input.commands_succeeded {
        return retryable(
            FailureClassification::AcceptanceFailed,
            "acceptance command failed",
            evidence_refs,
        );
    }
    if !required_artifacts_changed(input.step, &changed) {
        return retryable(
            FailureClassification::InadequateImplementation,
            "acceptance checks passed but the requested implementation was not evidenced in required artifacts",
            evidence_refs,
        );
    }
    CoordinatorReview {
        disposition: CoordinatorReviewDisposition::Accepted,
        classification: None,
        rationale: "independent coordinator review accepted the implementation against requested paths, artifacts, and evidence".to_string(),
        evidence_refs,
        repair_instruction: None,
    }
}

fn review_verification(input: ReviewInput<'_>, evidence_refs: Vec<String>) -> CoordinatorReview {
    if !input.missing_artifacts.is_empty() {
        return retryable(
            FailureClassification::ArtifactMissing,
            format!(
                "required artifacts missing: {}",
                input.missing_artifacts.join(", ")
            ),
            evidence_refs,
        );
    }
    if !input.commands_succeeded {
        return retryable(
            FailureClassification::AcceptanceFailed,
            "verification acceptance command failed",
            evidence_refs,
        );
    }
    CoordinatorReview {
        disposition: CoordinatorReviewDisposition::Accepted,
        classification: None,
        rationale: "verification commands passed; verification steps do not create commits"
            .to_string(),
        evidence_refs,
        repair_instruction: None,
    }
}

fn assert_allowed_paths(step: &CampaignStep, changed: &[String]) -> Result<(), String> {
    assert_authorized_paths(changed, &step.allowed_paths)
}

fn assert_required_changed_paths(step: &CampaignStep, changed: &[String]) -> Result<(), String> {
    for required in &step.required_changed_paths {
        if !required_prefix_satisfied(changed, required)? {
            return Err(format!(
                "required changed path not satisfied: {required}; a passing test suite is not sufficient"
            ));
        }
    }
    Ok(())
}

fn required_artifacts_changed(step: &CampaignStep, changed: &[String]) -> bool {
    if step.acceptance.required_artifacts.is_empty() {
        return true;
    }
    step.acceptance
        .required_artifacts
        .iter()
        .any(|artifact| required_prefix_satisfied(changed, artifact).unwrap_or(false))
}

fn retryable(
    classification: FailureClassification,
    rationale: impl Into<String>,
    evidence_refs: Vec<String>,
) -> CoordinatorReview {
    CoordinatorReview {
        disposition: CoordinatorReviewDisposition::RejectedRetryable,
        classification: Some(classification),
        rationale: rationale.into(),
        evidence_refs,
        repair_instruction: None,
    }
}

fn terminal(
    classification: FailureClassification,
    rationale: impl Into<String>,
    evidence_refs: Vec<String>,
) -> CoordinatorReview {
    CoordinatorReview {
        disposition: CoordinatorReviewDisposition::RejectedTerminal,
        classification: Some(classification),
        rationale: rationale.into(),
        evidence_refs,
        repair_instruction: None,
    }
}

#[cfg(test)]
mod tests {
    use super::ReviewInput;
    use super::looks_like_markdown_tool_call;
    use super::repair_instruction;
    use super::review_attempt;
    use crate::CampaignStep;
    use crate::CampaignStepKind;
    use crate::CoordinatorReviewDisposition;
    use crate::FailureClassification;
    use crate::GitEvidence;
    use crate::StepAcceptance;
    use crate::StepLimits;
    use rack_ai_domain::GitSha;

    #[test]
    fn detects_markdown_tool_calls() {
        let text = "I'll edit the file:\n```json\n{\"name\": \"write\", \"arguments\": {\"file_path\": \"src/lib.rs\"}}\n```\nCOMPLETE";
        assert!(looks_like_markdown_tool_call(text));
        assert!(!looks_like_markdown_tool_call("COMPLETE"));
    }

    #[test]
    fn rejects_passing_tests_without_required_change() {
        let step = sample_step();
        let evidence = GitEvidence::new(
            GitSha::new("a".repeat(40)).unwrap(),
            " M src/lib.rs".to_string(),
        )
        .with_changed_paths(vec!["src/lib.rs".to_string()])
        .with_diff_stat("src/lib.rs | 1 +\n".to_string());
        let review = review_attempt(ReviewInput {
            step: &step,
            evidence: &evidence,
            commands_succeeded: true,
            missing_artifacts: Vec::new(),
            implementer_output: Some("COMPLETE"),
            protocol_error: None,
            worker_error: None,
            tool_calls: 1,
            used_host_shell: false,
        });
        assert_eq!(
            review.disposition,
            CoordinatorReviewDisposition::RejectedRetryable
        );
        assert_eq!(review.classification, Some(FailureClassification::NoChange));
        let instruction = repair_instruction(&step, &review, evidence.diff_stat(), &[]);
        assert!(instruction.contains("src/domain/"));
        assert!(instruction.contains("Do not broaden allowed_paths"));
    }

    fn sample_step() -> CampaignStep {
        CampaignStep {
            id: "domain".to_string(),
            kind: CampaignStepKind::Implementation,
            task: "Add domain identifiers.".to_string(),
            allowed_paths: vec!["src/".to_string()],
            required_changed_paths: vec!["src/domain/".to_string()],
            acceptance: StepAcceptance {
                commands: vec![vec!["cargo".to_string(), "test".to_string()]],
                required_artifacts: vec!["src/domain/mod.rs".to_string()],
            },
            limits: StepLimits {
                timeout_seconds: 60,
                network: "disabled".to_string(),
            },
        }
    }
}
