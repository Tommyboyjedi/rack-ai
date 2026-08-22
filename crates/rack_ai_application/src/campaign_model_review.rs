use rack_ai_domain::AllowedPaths;
use serde::Serialize;

use crate::CampaignStep;
use crate::CommandEvidence;
use crate::CoordinatorReviewDisposition;
use crate::FailureClassification;
use crate::GitEvidence;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelReviewRequest {
    pub campaign_id: String,
    pub step_id: String,
    pub task: String,
    pub allowed_paths: Vec<String>,
    pub required_changed_paths: Vec<String>,
    pub worker_id: String,
    pub isolated_context: bool,
    pub diff: String,
    pub diff_stat: String,
    pub git_status: String,
    pub changed_paths: Vec<String>,
    pub command_summary: String,
    pub previous_rejection: Option<String>,
    pub timeout_seconds: u32,
}

impl ModelReviewRequest {
    pub fn from_step(
        campaign_id: &str,
        step: &CampaignStep,
        worker_id: &str,
        isolated_context: bool,
        evidence: &GitEvidence,
        commands: &[CommandEvidence],
        previous_rejection: Option<&str>,
        timeout_seconds: u32,
    ) -> Self {
        let command_summary = commands
            .iter()
            .map(|item| {
                format!(
                    "{} => exit {} timeout={}",
                    item.argv().join(" "),
                    item.exit_code(),
                    item.timed_out()
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        Self {
            campaign_id: campaign_id.to_string(),
            step_id: step.id.clone(),
            task: step.task.clone(),
            allowed_paths: step.allowed_paths.clone(),
            required_changed_paths: step.required_changed_paths.clone(),
            worker_id: worker_id.to_string(),
            isolated_context,
            diff: evidence.diff().to_string(),
            diff_stat: evidence.diff_stat().to_string(),
            git_status: evidence.status().to_string(),
            changed_paths: evidence.changed_paths().to_vec(),
            command_summary,
            previous_rejection: previous_rejection.map(|value| value.to_string()),
            timeout_seconds: timeout_seconds.max(1),
        }
    }

    pub fn prompt(&self) -> String {
        format!(
            "You are a read-only campaign coordinator. Do not write files or call tools.\nReview whether the implementation satisfies the requested step.\nCampaign: {}\nStep: {}\nWorker: {}\nFresh isolated context: {}\nTask: {}\nAllowed paths: {}\nRequired changed paths: {}\nChanged paths: {}\nGit status:\n{}\nDiff stat:\n{}\nDiff:\n{}\nAcceptance commands: {}\nPrevious rejection: {}\nReply with exactly one JSON object: {{\"disposition\":\"accepted|rejected_retryable|rejected_terminal\",\"classification\":null|\"inadequate_implementation|no_change|path_policy_failed\",\"rationale\":\"...\"}}.",
            self.campaign_id,
            self.step_id,
            self.worker_id,
            self.isolated_context,
            self.task,
            self.allowed_paths.join(", "),
            self.required_changed_paths.join(", "),
            self.changed_paths.join(", "),
            self.git_status,
            self.diff_stat,
            self.diff,
            self.command_summary,
            self.previous_rejection.as_deref().unwrap_or("none")
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelReviewResult {
    pub disposition: CoordinatorReviewDisposition,
    pub classification: Option<FailureClassification>,
    pub rationale: String,
    pub prompt: String,
    pub raw_output: String,
    pub used_host_shell: bool,
}

pub trait ImplementationReviewer {
    fn review(&self, request: &ModelReviewRequest) -> Result<ModelReviewResult, String>;
}

pub struct AcceptingReviewer;

impl ImplementationReviewer for AcceptingReviewer {
    fn review(&self, request: &ModelReviewRequest) -> Result<ModelReviewResult, String> {
        Ok(ModelReviewResult {
            disposition: CoordinatorReviewDisposition::Accepted,
            classification: None,
            rationale: "test reviewer accepted after deterministic gates".to_string(),
            prompt: request.prompt(),
            raw_output:
                "{\"disposition\":\"accepted\",\"classification\":null,\"rationale\":\"ok\"}"
                    .to_string(),
            used_host_shell: false,
        })
    }
}

pub struct RejectingReviewer {
    pub rationale: String,
}

impl ImplementationReviewer for RejectingReviewer {
    fn review(&self, request: &ModelReviewRequest) -> Result<ModelReviewResult, String> {
        let _allowed = AllowedPaths::new(
            request
                .allowed_paths
                .iter()
                .cloned()
                .map(rack_ai_domain::AllowedPath::new)
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        Ok(ModelReviewResult {
            disposition: CoordinatorReviewDisposition::RejectedRetryable,
            classification: Some(FailureClassification::InadequateImplementation),
            rationale: self.rationale.clone(),
            prompt: request.prompt(),
            raw_output: format!(
                "{{\"disposition\":\"rejected_retryable\",\"classification\":\"inadequate_implementation\",\"rationale\":\"{}\"}}",
                self.rationale
            ),
            used_host_shell: false,
        })
    }
}

pub fn parse_model_review_output(
    raw: &str,
) -> Result<
    (
        CoordinatorReviewDisposition,
        Option<FailureClassification>,
        String,
    ),
    String,
> {
    let json = extract_json_object(raw)
        .ok_or_else(|| "model review did not return a JSON object".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|error| error.to_string())?;
    let disposition = match value.get("disposition").and_then(|item| item.as_str()) {
        Some("accepted") => CoordinatorReviewDisposition::Accepted,
        Some("rejected_retryable") => CoordinatorReviewDisposition::RejectedRetryable,
        Some("rejected_terminal") => CoordinatorReviewDisposition::RejectedTerminal,
        other => {
            return Err(format!(
                "model review returned invalid disposition: {other:?}"
            ));
        }
    };
    let classification = match value.get("classification").and_then(|item| item.as_str()) {
        None | Some("") | Some("null") => None,
        Some("inadequate_implementation") => Some(FailureClassification::InadequateImplementation),
        Some("no_change") => Some(FailureClassification::NoChange),
        Some("path_policy_failed") => Some(FailureClassification::PathPolicyFailed),
        Some(other) => Some(FailureClassification::InadequateImplementation).filter(|_| {
            let _ = other;
            true
        }),
    };
    let rationale = value
        .get("rationale")
        .and_then(|item| item.as_str())
        .unwrap_or("model review returned no rationale")
        .to_string();
    Ok((disposition, classification, rationale))
}

fn extract_json_object(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end >= start {
        Some(raw[start..=end].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::parse_model_review_output;
    use crate::CoordinatorReviewDisposition;
    use crate::FailureClassification;

    #[test]
    fn parses_fenced_review_json() {
        let raw = "```json\n{\"disposition\":\"rejected_retryable\",\"classification\":\"inadequate_implementation\",\"rationale\":\"missing domain type\"}\n```";
        let (disposition, classification, rationale) = parse_model_review_output(raw).unwrap();
        assert_eq!(disposition, CoordinatorReviewDisposition::RejectedRetryable);
        assert_eq!(
            classification,
            Some(FailureClassification::InadequateImplementation)
        );
        assert!(rationale.contains("missing domain type"));
    }
}
