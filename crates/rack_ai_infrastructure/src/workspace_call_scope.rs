//! The trusted runner owns this lifetime; a dropped model HTTP connection does not.
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
pub struct WorkspaceCallRequest<'a> {
    pub endpoint: &'a str,
    pub workdir: &'a Path,
    pub task: &'a str,
    pub deadline_ms: u64,
}
pub struct WorkspaceDeadline {
    pub instant: Instant,
    pub unix_ms: u64,
}
impl WorkspaceDeadline {
    pub fn new(timeout_seconds: u32) -> Result<Self, String> {
        let duration = Duration::from_secs(u64::from(timeout_seconds.max(1)));
        // Capture wall time first: the durable fence cannot outlive the runner's budget.
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            instant: Instant::now() + duration,
            unix_ms: (wall + duration).as_millis() as u64,
        })
    }
}
pub struct WorkspaceCallScope {
    endpoint: String,
    control_endpoint: String,
}
impl WorkspaceCallScope {
    pub fn open(request: WorkspaceCallRequest<'_>) -> Result<Option<Self>, String> {
        if !request.endpoint.contains("/scoped/") {
            return Ok(None);
        }
        let _fence = crate::endpoint_fence::EndpointFence::local(request.endpoint)?;
        let encoded =
            serde_json::to_vec(&(request.workdir, request.task)).map_err(|e| e.to_string())?;
        let key = format!("{:x}", Sha256::digest(encoded));
        let base = request.endpoint.trim_end_matches('/');
        let scope = Self {
            endpoint: format!("{base}/calls/{key}"),
            control_endpoint: format!("{base}/scopes/{key}"),
        };
        scope
            .control(serde_json::json!({"operation":"open", "deadline_ms":request.deadline_ms}))?;
        Ok(Some(scope))
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn close(&self) -> Result<(), String> {
        self.control(serde_json::json!({"operation":"close"}))
    }
    fn control(&self, body: serde_json::Value) -> Result<(), String> {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(CONTROL_TIMEOUT))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut response = agent
            .post(&self.control_endpoint)
            .send_json(&body)
            .map_err(|e| format!("workspace scope control persistence unconfirmed: {e}"))?;
        if response.status().as_u16() != 204 {
            let detail = response
                .body_mut()
                .with_config()
                .limit(4096)
                .read_to_string()
                .map_err(|e| e.to_string())?;
            return Err(format!(
                "workspace scope control persistence unconfirmed: {} {detail}",
                response.status()
            ));
        }
        Ok(())
    }
}
