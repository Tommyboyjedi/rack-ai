use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use rack_ai_domain::WorkUnitComplexity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericCapability {
    Reasoning,
    Coding,
    Visual,
    Audio,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericPriority {
    Low,
    Medium,
    High,
    Paramount,
}

impl GenericPriority {
    pub fn permits(self, requested: Self) -> bool {
        requested <= self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GenericRoutingHeader {
    pub source_system: String,
    pub work_id: String,
    pub submission_id: String,
    pub idempotency_key: String,
    pub required_capabilities: Vec<GenericCapability>,
    pub priority: GenericPriority,
}

impl GenericRoutingHeader {
    pub fn new(
        source_system: String,
        work_id: String,
        submission_id: String,
        idempotency_key: String,
        required_capabilities: Vec<GenericCapability>,
        priority: GenericPriority,
    ) -> Result<Self, String> {
        let source_system = required_identity(source_system, "source_system")?.to_ascii_lowercase();
        let work_id = required_identity(work_id, "work_id")?;
        let submission_id = required_identity(submission_id, "submission_id")?;
        let idempotency_key = required_identity(idempotency_key, "idempotency_key")?;
        let provided_count = required_capabilities.len();
        if provided_count == 0 {
            return Err("required_capabilities must not be empty".to_string());
        }
        let required_capabilities = required_capabilities.into_iter().collect::<BTreeSet<_>>();
        if required_capabilities.len() != provided_count {
            return Err("required_capabilities must be unique".to_string());
        }
        Ok(Self {
            source_system,
            work_id,
            submission_id,
            idempotency_key,
            required_capabilities: required_capabilities.into_iter().collect(),
            priority,
        })
    }
}

fn required_identity(value: String, field: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(value)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GenericSourceAdmissionPolicy {
    pub source_system: String,
    pub max_priority: GenericPriority,
}

impl GenericSourceAdmissionPolicy {
    pub fn matches(&self, source_system: &str) -> bool {
        self.source_system == "*" || self.source_system.eq_ignore_ascii_case(source_system)
    }
    pub fn admits(&self, priority: GenericPriority) -> bool {
        self.max_priority.permits(priority)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericQualificationStatus {
    Qualified,
    QualifiedWithConstraints,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GenericModelEligibilityProfile {
    pub model_profile_id: String,
    pub capabilities: Vec<GenericCapability>,
    pub max_complexity: WorkUnitComplexity,
    pub large_context_eligible: bool,
    pub qualification_status: GenericQualificationStatus,
    pub qualification_evidence_refs: Vec<String>,
    pub profile_version: String,
    pub execution_constraints: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericWorkerIneligibilityReason {
    WorkerDisabled,
    UnsupportedHarness,
    ModelUnavailable,
    CapabilityUnsupported,
    ComplexityUnqualified,
    LargeContextUnsupported,
    EligibilityProfileMissing,
    ModelBindingMissing,
    ResourceBindingMissing,
    TemporarilyUnavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GenericWorkerIneligibility {
    pub worker_id: String,
    pub reason: GenericWorkerIneligibilityReason,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericSelectionReason {
    LeastScarceSufficient,
    OnlyEligible,
    HigherThroughput,
    WarmModel,
    OperatorPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericResourceAvailability {
    Available,
    TemporarilyUnavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GenericResourceAvailabilityEvidence {
    pub worker_id: String,
    pub resource_id: String,
    pub availability: GenericResourceAvailability,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GenericWorkerSelectionDecision {
    pub decision_id: String,
    pub submission_id: String,
    pub work_id: String,
    pub idempotency_key: String,
    pub source_system: String,
    pub requested_capabilities: Vec<GenericCapability>,
    pub requested_complexity: WorkUnitComplexity,
    pub requested_large_context: bool,
    pub requested_priority: GenericPriority,
    pub eligible_worker_ids: Vec<String>,
    pub ineligible_workers_with_generic_reasons: Vec<GenericWorkerIneligibility>,
    pub selected_worker_id: Option<String>,
    pub selection_reason: Option<GenericSelectionReason>,
    pub model_profile_version: Option<String>,
    pub qualification_evidence_refs: Vec<String>,
    pub policy_version: String,
    pub resource_availability_evidence: Vec<GenericResourceAvailabilityEvidence>,
    pub created_at: String,
}

impl GenericWorkerSelectionDecision {
    pub fn new(
        header: &GenericRoutingHeader,
        complexity: WorkUnitComplexity,
        large_context: bool,
    ) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();
        Self {
            decision_id: format!("selection-{}", header.submission_id),
            submission_id: header.submission_id.clone(),
            work_id: header.work_id.clone(),
            idempotency_key: header.idempotency_key.clone(),
            source_system: header.source_system.clone(),
            requested_capabilities: header.required_capabilities.clone(),
            requested_complexity: complexity,
            requested_large_context: large_context,
            requested_priority: header.priority,
            eligible_worker_ids: Vec::new(),
            ineligible_workers_with_generic_reasons: Vec::new(),
            selected_worker_id: None,
            selection_reason: None,
            model_profile_version: None,
            qualification_evidence_refs: Vec::new(),
            policy_version: "rack-ai/capability-routing/v1".to_string(),
            resource_availability_evidence: Vec::new(),
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonicalizes_capability_order_and_rejects_duplicates() {
        let header = GenericRoutingHeader::new(
            "ATHBA".into(),
            "w".into(),
            "s".into(),
            "i".into(),
            vec![GenericCapability::Coding, GenericCapability::Reasoning],
            GenericPriority::Low,
        )
        .unwrap();
        assert_eq!(header.source_system, "athba");
        assert_eq!(
            header.required_capabilities,
            vec![GenericCapability::Reasoning, GenericCapability::Coding]
        );
        assert!(
            GenericRoutingHeader::new(
                "athba".into(),
                "w".into(),
                "s".into(),
                "i".into(),
                vec![GenericCapability::Coding, GenericCapability::Coding],
                GenericPriority::Low
            )
            .is_err()
        );
    }
}
