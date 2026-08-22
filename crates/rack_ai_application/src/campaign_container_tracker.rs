use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde::Serialize;

use crate::CampaignLock;
use crate::CampaignStatus;
use crate::atomic_write;

pub trait ContainerLifecycleObserver: Send + Sync {
    fn container_started(&self, container_id: &str) -> Result<(), String>;
    fn container_finished(&self) -> Result<(), String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActiveContainerRecord {
    pub campaign_id: String,
    pub step_id: Option<String>,
    pub action: String,
    pub container_id: String,
    pub recorded_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveContainerScope {
    campaign_id: String,
    step_id: Option<String>,
    action: String,
}

pub struct CampaignContainerTracker {
    state_root: PathBuf,
    scope: Mutex<Option<ActiveContainerScope>>,
}

pub struct CampaignContainerScopeGuard<'a> {
    tracker: &'a CampaignContainerTracker,
}

impl Drop for CampaignContainerScopeGuard<'_> {
    fn drop(&mut self) {
        self.tracker.clear_scope();
    }
}

impl CampaignContainerTracker {
    pub fn new(state_root: PathBuf) -> Self {
        Self {
            state_root,
            scope: Mutex::new(None),
        }
    }

    pub fn bind(
        &self,
        campaign_id: &str,
        step_id: Option<&str>,
        action: &str,
    ) -> CampaignContainerScopeGuard<'_> {
        let mut scope = self.scope.lock().unwrap();
        *scope = Some(ActiveContainerScope {
            campaign_id: campaign_id.to_string(),
            step_id: step_id.map(str::to_string),
            action: action.to_string(),
        });
        CampaignContainerScopeGuard { tracker: self }
    }

    pub fn cleanup_campaign_container(
        &self,
        campaign_id: &str,
        command: &str,
    ) -> Result<Option<String>, String> {
        let campaign_dir = self.campaign_dir(campaign_id);
        let _lock = CampaignLock::acquire(&campaign_dir)?;
        let state_path = self.state_path(campaign_id);
        let active_path = self.active_container_path(campaign_id);
        let state = load_state(&state_path)?;
        let container_id = if active_path.exists() {
            let content = fs::read_to_string(&active_path).map_err(|error| error.to_string())?;
            let record = serde_json::from_str::<ActiveContainerRecord>(&content)
                .map_err(|error| error.to_string())?;
            Some(record.container_id)
        } else {
            state
                .as_ref()
                .and_then(|value| value.active_container_id.clone())
        };
        let Some(container_id) = container_id else {
            if active_path.exists() {
                fs::remove_file(&active_path).map_err(|error| error.to_string())?;
            }
            if let Some(mut state) = state {
                if state.active_container_id.is_some() {
                    state.active_container_id = None;
                    save_state(&state_path, &state)?;
                }
            }
            return Ok(None);
        };
        run_container_cleanup(command, &container_id)?;
        if active_path.exists() {
            fs::remove_file(&active_path).map_err(|error| error.to_string())?;
        }
        if let Some(mut state) = state {
            state.active_container_id = None;
            save_state(&state_path, &state)?;
        }
        Ok(Some(container_id))
    }

    fn clear_scope(&self) {
        if let Ok(mut scope) = self.scope.lock() {
            *scope = None;
        }
    }

    fn campaign_dir(&self, campaign_id: &str) -> PathBuf {
        self.state_root
            .join("state")
            .join("campaigns")
            .join(campaign_id)
    }

    fn state_path(&self, campaign_id: &str) -> PathBuf {
        self.campaign_dir(campaign_id).join("state.json")
    }

    fn active_container_path(&self, campaign_id: &str) -> PathBuf {
        self.campaign_dir(campaign_id).join("active-container.json")
    }
}

impl ContainerLifecycleObserver for CampaignContainerTracker {
    fn container_started(&self, container_id: &str) -> Result<(), String> {
        let scope = self.scope.lock().unwrap().clone();
        let Some(scope) = scope else {
            return Ok(());
        };
        let campaign_dir = self.campaign_dir(&scope.campaign_id);
        let _lock = CampaignLock::acquire(&campaign_dir)?;
        let state_path = self.state_path(&scope.campaign_id);
        let Some(mut state) = load_state(&state_path)? else {
            return Err(format!("campaign state missing for {}", scope.campaign_id));
        };
        state.active_container_id = Some(container_id.to_string());
        save_state(&state_path, &state)?;
        let record = ActiveContainerRecord {
            campaign_id: scope.campaign_id.clone(),
            step_id: scope.step_id.clone(),
            action: scope.action,
            container_id: container_id.to_string(),
            recorded_at: now_text(),
        };
        let json = serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?;
        atomic_write(
            &self.active_container_path(&record.campaign_id),
            &format!("{json}\n"),
        )
    }

    fn container_finished(&self) -> Result<(), String> {
        let scope = self.scope.lock().unwrap().clone();
        let Some(scope) = scope else {
            return Ok(());
        };
        let campaign_dir = self.campaign_dir(&scope.campaign_id);
        let _lock = CampaignLock::acquire(&campaign_dir)?;
        let state_path = self.state_path(&scope.campaign_id);
        if let Some(mut state) = load_state(&state_path)? {
            if state.active_container_id.is_some() {
                state.active_container_id = None;
                save_state(&state_path, &state)?;
            }
        }
        let active_path = self.active_container_path(&scope.campaign_id);
        if active_path.exists() {
            fs::remove_file(active_path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn load_state(path: &PathBuf) -> Result<Option<CampaignStatus>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let campaign_path = path.parent().map(|value| value.join("campaign.json"));
    crate::load_campaign_status_compatible(path, campaign_path.as_deref())
}

fn save_state(path: &PathBuf, state: &CampaignStatus) -> Result<(), String> {
    let json = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
    atomic_write(path, &format!("{json}\n"))
}

fn run_container_cleanup(command: &str, container_id: &str) -> Result<(), String> {
    let stop = Command::new(command)
        .args(["stop", "--time", "0", container_id])
        .output()
        .map_err(|error| error.to_string())?;
    if !stop.status.success() && !stderr_not_found(&stop.stderr) {
        return Err(format!(
            "failed to stop container {container_id}: {}",
            String::from_utf8_lossy(&stop.stderr).trim()
        ));
    }
    let remove = Command::new(command)
        .args(["rm", "-f", container_id])
        .output()
        .map_err(|error| error.to_string())?;
    if !remove.status.success() && !stderr_not_found(&remove.stderr) {
        return Err(format!(
            "failed to remove container {container_id}: {}",
            String::from_utf8_lossy(&remove.stderr).trim()
        ));
    }
    Ok(())
}

fn stderr_not_found(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr).to_lowercase();
    text.contains("no such container") || text.contains("no container")
}

fn now_text() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
