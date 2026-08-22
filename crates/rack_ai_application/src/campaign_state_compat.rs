use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::AttemptKind;
use crate::Campaign;
use crate::CampaignStatus;

pub fn load_campaign_status_compatible(
    state_path: &Path,
    campaign_path: Option<&Path>,
) -> Result<Option<CampaignStatus>, String> {
    if !state_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(state_path).map_err(|error| error.to_string())?;
    let state = deserialize_campaign_status_compatible(&content, campaign_path)?;
    Ok(Some(state))
}

pub fn deserialize_campaign_status_compatible(
    content: &str,
    campaign_path: Option<&Path>,
) -> Result<CampaignStatus, String> {
    match serde_json::from_str::<CampaignStatus>(content) {
        Ok(state) => Ok(state),
        Err(original_error) => {
            let mut value =
                serde_json::from_str::<Value>(content).map_err(|error| error.to_string())?;
            migrate_missing_attempt_kinds(&mut value, campaign_path)
                .map_err(|error| format!("{original_error}; {error}"))?;
            serde_json::from_value(value).map_err(|error| {
                format!("{original_error}; compatibility migration failed: {error}")
            })
        }
    }
}

fn migrate_missing_attempt_kinds(
    value: &mut Value,
    campaign_path: Option<&Path>,
) -> Result<(), String> {
    let Some(steps) = value.get_mut("steps").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let campaign_step_kinds = load_campaign_step_kinds(campaign_path);
    for step in steps {
        let Some(step_object) = step.as_object_mut() else {
            continue;
        };
        let step_id = step_object
            .get("step_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let step_kind = step_object
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| campaign_step_kinds.get(step_id).cloned());
        let Some(attempts) = step_object
            .get_mut("attempts")
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for attempt in attempts {
            let Some(attempt_object) = attempt.as_object_mut() else {
                continue;
            };
            if attempt_object.contains_key("kind") {
                continue;
            }
            let inferred = infer_attempt_kind(step_kind.as_deref(), attempt_object)?;
            attempt_object.insert(
                "kind".to_string(),
                Value::String(attempt_kind_name(inferred).to_string()),
            );
        }
    }
    Ok(())
}

fn infer_attempt_kind(
    step_kind: Option<&str>,
    attempt_object: &serde_json::Map<String, Value>,
) -> Result<AttemptKind, String> {
    if attempt_object
        .get("fallback_of")
        .is_some_and(|value| !value.is_null())
    {
        return Ok(AttemptKind::Fallback);
    }
    if attempt_object
        .get("repair_of")
        .is_some_and(|value| !value.is_null())
    {
        return Ok(AttemptKind::Repair);
    }
    match step_kind {
        Some("verification") => Ok(AttemptKind::Verification),
        Some("implementation") => Ok(AttemptKind::Primary),
        Some(other) => Err(format!(
            "cannot infer missing attempt kind from step kind {other}"
        )),
        None => Err(
            "cannot infer missing attempt kind without step kind or repair/fallback linkage"
                .to_string(),
        ),
    }
}

fn load_campaign_step_kinds(campaign_path: Option<&Path>) -> BTreeMap<String, String> {
    let Some(path) = campaign_path else {
        return BTreeMap::new();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(campaign) = serde_json::from_str::<Campaign>(&content) else {
        return BTreeMap::new();
    };
    campaign
        .steps
        .into_iter()
        .map(|step| (step.id, campaign_step_kind_name(step.kind).to_string()))
        .collect()
}

fn attempt_kind_name(kind: AttemptKind) -> &'static str {
    match kind {
        AttemptKind::Primary => "primary",
        AttemptKind::Repair => "repair",
        AttemptKind::Fallback => "fallback",
        AttemptKind::Verification => "verification",
    }
}

fn campaign_step_kind_name(kind: crate::CampaignStepKind) -> &'static str {
    match kind {
        crate::CampaignStepKind::Implementation => "implementation",
        crate::CampaignStepKind::Verification => "verification",
    }
}
