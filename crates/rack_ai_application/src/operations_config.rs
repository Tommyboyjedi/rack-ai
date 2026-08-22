use serde::Deserialize;
use serde::Serialize;

const OPERATIONS_SCHEMA_VERSION: &str = "rack-ai/operations/v1";
const MIN_SCAN_INTERVAL_SECONDS: u64 = 10;
const MIN_RETENTION_SECONDS: u64 = 3600;
const MIN_RETAIN_COUNT: usize = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsConfig {
    pub schema_version: String,
    pub supervisor: SupervisorConfig,
    pub retention: RetentionConfig,
}

impl OperationsConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != OPERATIONS_SCHEMA_VERSION {
            return Err(format!(
                "unsupported operations schema_version: {}",
                self.schema_version
            ));
        }
        self.supervisor.validate()?;
        self.retention.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SupervisorConfig {
    pub scan_interval_seconds: u64,
    pub resume_running_campaigns: bool,
    pub podman_command: String,
}

impl SupervisorConfig {
    fn validate(&self) -> Result<(), String> {
        if self.scan_interval_seconds < MIN_SCAN_INTERVAL_SECONDS {
            return Err(format!(
                "scan_interval_seconds must be at least {MIN_SCAN_INTERVAL_SECONDS}"
            ));
        }
        if self.podman_command.trim().is_empty() {
            return Err("podman_command cannot be empty".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetentionConfig {
    pub max_terminal_campaign_age_seconds: u64,
    pub retain_terminal_campaigns: usize,
    pub max_auxiliary_artifact_age_seconds: u64,
    pub retain_auxiliary_artifacts: usize,
}

impl RetentionConfig {
    fn validate(&self) -> Result<(), String> {
        validate_seconds(
            self.max_terminal_campaign_age_seconds,
            "max_terminal_campaign_age_seconds",
        )?;
        validate_count(self.retain_terminal_campaigns, "retain_terminal_campaigns")?;
        validate_seconds(
            self.max_auxiliary_artifact_age_seconds,
            "max_auxiliary_artifact_age_seconds",
        )?;
        validate_count(
            self.retain_auxiliary_artifacts,
            "retain_auxiliary_artifacts",
        )?;
        Ok(())
    }
}

fn validate_seconds(value: u64, field: &str) -> Result<(), String> {
    if value < MIN_RETENTION_SECONDS {
        return Err(format!("{field} must be at least {MIN_RETENTION_SECONDS}"));
    }
    Ok(())
}

fn validate_count(value: usize, field: &str) -> Result<(), String> {
    if value < MIN_RETAIN_COUNT {
        return Err(format!("{field} must be at least {MIN_RETAIN_COUNT}"));
    }
    Ok(())
}
