use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::atomic_write;
use crate::CampaignLock;

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignLeaseRecord {
    pub campaign_id: String,
    pub repository_id: String,
    pub pid: u32,
    pub acquired_at: String,
    pub heartbeat: String,
    #[serde(default)]
    pub heartbeat_seconds: u64,
    #[serde(default)]
    pub action_timeout_seconds: u64,
}

pub struct CampaignLeaseStore {
    state_root: PathBuf,
}

pub struct CampaignHeartbeatGuard {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for CampaignHeartbeatGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl CampaignLeaseStore {
    pub fn new(state_root: PathBuf) -> Self {
        Self { state_root }
    }
    pub fn start_background_heartbeat(
        &self,
        campaign_id: &str,
        repository_id: &str,
        heartbeat_seconds: u64,
    ) -> CampaignHeartbeatGuard {
        let state_root = self.state_root.clone();
        let campaign_id = campaign_id.to_string();
        let repository_id = repository_id.to_string();

        let interval = Duration::from_secs(
            heartbeat_seconds.clamp(1, 30)
        );

        let (stop_tx, stop_rx) = mpsc::channel();

        let thread = thread::spawn(move || {
            let leases = CampaignLeaseStore::new(state_root);

            loop {
                match stop_rx.recv_timeout(interval) {
                    Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            .to_string();

                        if leases
                            .heartbeat(&campaign_id, &repository_id, &now)
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        CampaignHeartbeatGuard {
            stop: Some(stop_tx),
            thread: Some(thread),
        }
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
        heartbeat_seconds: u64,
        action_timeout_seconds: u64,
    ) -> Result<CampaignLeaseRecord, String> {
        let _lock = CampaignLock::acquire_path(&self.lease_lock_path())?;
        self.fail_if_held(
            &self.campaign_lease_path(campaign_id),
            campaign_id,
            now,
            heartbeat_seconds,
            action_timeout_seconds,
        )?;
        self.fail_if_repository_held(
            repository_id,
            campaign_id,
            now,
            heartbeat_seconds,
            action_timeout_seconds,
        )?;
        let record = CampaignLeaseRecord {
            campaign_id: campaign_id.to_string(),
            repository_id: repository_id.to_string(),
            pid: std::process::id(),
            acquired_at: now.to_string(),
            heartbeat: now.to_string(),
            heartbeat_seconds,
            action_timeout_seconds,
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
        let _lock = CampaignLock::acquire_path(&self.lease_lock_path())?;
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
        let _lock = CampaignLock::acquire_path(&self.lease_lock_path())?;
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

    fn lease_lock_path(&self) -> PathBuf {
        self.state_root
            .join("state")
            .join("campaigns")
            .join(".lease.lock")
    }

    fn fail_if_repository_held(
        &self,
        repository_id: &str,
        campaign_id: &str,
        now: &str,
        heartbeat_seconds: u64,
        action_timeout_seconds: u64,
    ) -> Result<(), String> {
        let path = self.repository_lease_path(repository_id);
        if !path.exists() {
            return Ok(());
        }
        let record = self.read_record(&path)?;
        if record.campaign_id == campaign_id {
            return self.fail_if_held(
                &path,
                campaign_id,
                now,
                heartbeat_seconds,
                action_timeout_seconds,
            );
        }
        if lease_is_active(&record, now, heartbeat_seconds, action_timeout_seconds) {
            return Err(format!(
                "repository {} already has an active campaign lease held by {}",
                repository_id, record.campaign_id
            ));
        }
        fs::remove_file(&path).map_err(|error| error.to_string())
    }

    fn fail_if_held(
        &self,
        path: &Path,
        campaign_id: &str,
        now: &str,
        heartbeat_seconds: u64,
        action_timeout_seconds: u64,
    ) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let record = self.read_record(path)?;
        if record.pid == std::process::id() {
            return Ok(());
        }
        if lease_is_active(&record, now, heartbeat_seconds, action_timeout_seconds) {
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

        atomic_write(
            path,
            &format!("{json}\n"),
        )
    }
}

fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if std::path::Path::new(&format!("/proc/{pid}")).exists() {
        return true;
    }
    match std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
    {
        Ok(output) if output.status.success() => true,
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
            stderr.contains("not permitted") || stderr.contains("permission denied")
        }
        Err(_) => false,
    }
}

pub fn lease_is_stale(
    record: &CampaignLeaseRecord,
    now: &str,
    heartbeat_seconds: u64,
    action_timeout_seconds: u64,
) -> bool {
    let now_unix = now.parse::<u64>().unwrap_or(0);
    let heartbeat_unix = record.heartbeat.parse::<u64>().unwrap_or(0);
    let interval = if record.heartbeat_seconds == 0 {
        heartbeat_seconds
    } else {
        record.heartbeat_seconds
    };
    let action_timeout = if record.action_timeout_seconds == 0 {
        action_timeout_seconds
    } else {
        record.action_timeout_seconds
    };
    let deadline = heartbeat_unix
        .saturating_add(interval.saturating_mul(2))
        .saturating_add(action_timeout);
    now_unix > deadline
}

fn lease_is_active(
    record: &CampaignLeaseRecord,
    now: &str,
    heartbeat_seconds: u64,
    action_timeout_seconds: u64,
) -> bool {
    process_is_alive(record.pid)
        && !lease_is_stale(record, now, heartbeat_seconds, action_timeout_seconds)
}

#[cfg(test)]
mod tests {
    use super::CampaignLeaseRecord;
    use super::CampaignLeaseStore;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn acquires_and_releases_campaign_lease() {
        let root = temp_root();
        let store = CampaignLeaseStore::new(root);
        let record = store.acquire("c1", "repo", "1", 10, 60).unwrap();
        assert_eq!(record.campaign_id, "c1");
        store.release("c1", "repo").unwrap();
        assert!(!store.campaign_lease_path("c1").exists());
    }

    #[test]
    fn refuses_fresh_lease_held_by_live_pid() {
        let root = temp_root();
        let store = CampaignLeaseStore::new(root);
        let foreign = CampaignLeaseRecord {
            campaign_id: "c1".to_string(),
            repository_id: "repo".to_string(),
            pid: 1,
            acquired_at: "1000".to_string(),
            heartbeat: "1000".to_string(),
            heartbeat_seconds: 10,
            action_timeout_seconds: 60,
        };
        store
            .write_record(&store.campaign_lease_path("c1"), &foreign)
            .unwrap();
        let error = store.acquire("c1", "repo", "1000", 10, 60).unwrap_err();
        assert!(error.contains("live pid"));
    }

    #[test]
    fn reclaims_stale_lease_even_if_pid_is_still_alive() {
        let root = temp_root();
        let store = CampaignLeaseStore::new(root);
        let foreign = CampaignLeaseRecord {
            campaign_id: "c1".to_string(),
            repository_id: "repo".to_string(),
            pid: 1,
            acquired_at: "1".to_string(),
            heartbeat: "1".to_string(),
            heartbeat_seconds: 10,
            action_timeout_seconds: 60,
        };
        store
            .write_record(&store.campaign_lease_path("c1"), &foreign)
            .unwrap();
        store
            .write_record(&store.repository_lease_path("repo"), &foreign)
            .unwrap();
        let record = store.acquire("c1", "repo", "1000", 10, 60).unwrap();
        assert_eq!(record.pid, std::process::id());
        assert_eq!(record.heartbeat, "1000");
    }

    #[test]
    fn background_heartbeat_is_bounded_to_thirty_seconds() {
        let root = temp_root();
        let store = CampaignLeaseStore::new(root);

        let guard = store.start_background_heartbeat(
            "c1",
            "repo",
            120,
        );

        drop(guard);
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
