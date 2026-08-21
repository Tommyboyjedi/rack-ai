use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignLeaseRecord {
    pub campaign_id: String,
    pub repository_id: String,
    pub pid: u32,
    pub acquired_at: String,
    pub heartbeat: String,
}

pub struct CampaignLeaseStore {
    state_root: PathBuf,
}

impl CampaignLeaseStore {
    pub fn new(state_root: PathBuf) -> Self {
        Self { state_root }
    }

    pub fn campaign_lease_path(&self, campaign_id: &str) -> PathBuf {
        self.state_root
            .join("state")
            .join("campaigns")
            .join(campaign_id)
            .join("lease.json")
    }

    pub fn repository_lease_path(&self, repository_id: &str) -> PathBuf {
        self.state_root
            .join("state")
            .join("campaigns")
            .join(".repository-leases")
            .join(format!("{repository_id}.json"))
    }

    pub fn acquire(
        &self,
        campaign_id: &str,
        repository_id: &str,
        now: &str,
    ) -> Result<CampaignLeaseRecord, String> {
        self.fail_if_held(&self.campaign_lease_path(campaign_id), campaign_id)?;
        self.fail_if_repository_held(repository_id, campaign_id)?;
        let record = CampaignLeaseRecord {
            campaign_id: campaign_id.to_string(),
            repository_id: repository_id.to_string(),
            pid: std::process::id(),
            acquired_at: now.to_string(),
            heartbeat: now.to_string(),
        };
        self.write_record(&self.campaign_lease_path(campaign_id), &record)?;
        self.write_record(&self.repository_lease_path(repository_id), &record)?;
        Ok(record)
    }

    pub fn heartbeat(
        &self,
        campaign_id: &str,
        repository_id: &str,
        now: &str,
    ) -> Result<(), String> {
        let path = self.campaign_lease_path(campaign_id);
        if !path.exists() {
            return Err(format!("campaign lease missing: {campaign_id}"));
        }
        let mut record = self.read_record(&path)?;
        if record.pid != std::process::id() {
            return Err(format!("campaign lease is owned by pid {}", record.pid));
        }
        record.heartbeat = now.to_string();
        self.write_record(&path, &record)?;
        self.write_record(&self.repository_lease_path(repository_id), &record)?;
        Ok(())
    }

    pub fn release(&self, campaign_id: &str, repository_id: &str) -> Result<(), String> {
        let campaign_path = self.campaign_lease_path(campaign_id);
        if campaign_path.exists() {
            fs::remove_file(&campaign_path).map_err(|error| error.to_string())?;
        }
        let repo_path = self.repository_lease_path(repository_id);
        if repo_path.exists() {
            if let Ok(record) = self.read_record(&repo_path) {
                if record.campaign_id == campaign_id {
                    fs::remove_file(&repo_path).map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(())
    }

    fn fail_if_repository_held(
        &self,
        repository_id: &str,
        campaign_id: &str,
    ) -> Result<(), String> {
        let path = self.repository_lease_path(repository_id);
        if !path.exists() {
            return Ok(());
        }
        let record = self.read_record(&path)?;
        if record.campaign_id == campaign_id {
            return self.fail_if_held(&path, campaign_id);
        }
        if process_is_alive(record.pid) {
            return Err(format!(
                "repository {} already has an active campaign lease held by {}",
                repository_id, record.campaign_id
            ));
        }
        fs::remove_file(&path).map_err(|error| error.to_string())
    }

    fn fail_if_held(&self, path: &Path, campaign_id: &str) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let record = self.read_record(path)?;
        if record.pid == std::process::id() {
            return Ok(());
        }
        if process_is_alive(record.pid) {
            return Err(format!(
                "campaign {campaign_id} lease is held by live pid {}",
                record.pid
            ));
        }
        fs::remove_file(path).map_err(|error| error.to_string())
    }

    fn read_record(&self, path: &Path) -> Result<CampaignLeaseRecord, String> {
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        serde_json::from_str(&content).map_err(|error| error.to_string())
    }

    fn write_record(&self, path: &Path, record: &CampaignLeaseRecord) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let json = serde_json::to_string_pretty(record).map_err(|error| error.to_string())?;
        fs::write(path, format!("{json}\n")).map_err(|error| error.to_string())
    }
}

fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    match std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::CampaignLeaseStore;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn acquires_and_releases_campaign_lease() {
        let root = temp_root();
        let store = CampaignLeaseStore::new(root);
        let record = store.acquire("c1", "repo", "1").unwrap();
        assert_eq!(record.campaign_id, "c1");
        store.release("c1", "repo").unwrap();
        assert!(!store.campaign_lease_path("c1").exists());
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-lease-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
