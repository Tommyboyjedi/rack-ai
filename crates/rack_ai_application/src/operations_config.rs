use serde::Deserialize;
use serde::Serialize;

const OPERATIONS_SCHEMA_VERSION: &str = "rack-ai/operations/v1";
const MIN_SCAN_INTERVAL_SECONDS: u64 = 10;
const MIN_TERMINAL_RETENTION_SECONDS: u64 = 3600;
const MIN_TERMINAL_CAMPAIGNS_TO_KEEP: usize = 1;

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
}

impl SupervisorConfig {
    fn validate(&self) -> Result<(), String> {
        if self.scan_interval_seconds < MIN_SCAN_INTERVAL_SECONDS {
            return Err(format!(
                "scan_interval_seconds must be at least {MIN_SCAN_INTERVAL_SECONDS}"
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetentionConfig {
    pub max_terminal_campaign_age_seconds: u64,
    pub retain_terminal_campaigns: usize,
}

impl RetentionConfig {
    fn validate(&self) -> Result<(), String> {
        if self.max_terminal_campaign_age_seconds < MIN_TERMINAL_RETENTION_SECONDS {
            return Err(format!(
                "max_terminal_campaign_age_seconds must be at least {MIN_TERMINAL_RETENTION_SECONDS}"
            ));
        }
        if self.retain_terminal_campaigns < MIN_TERMINAL_CAMPAIGNS_TO_KEEP {
            return Err(format!(
                "retain_terminal_campaigns must be at least {MIN_TERMINAL_CAMPAIGNS_TO_KEEP}"
            ));
        }
        Ok(())
    }
}
