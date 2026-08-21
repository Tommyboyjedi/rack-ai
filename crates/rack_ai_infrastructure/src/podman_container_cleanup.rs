use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

pub struct PodmanContainerCleanup {
    command: String,
    cidfile: PathBuf,
}

impl PodmanContainerCleanup {
    pub fn new(command: String, cidfile: PathBuf) -> Self {
        Self { command, cidfile }
    }

    pub fn cidfile(&self) -> &Path {
        self.cidfile.as_path()
    }

    pub fn stop_and_remove(&self) {
        if let Some(id) = self.container_id() {
            let _ = Command::new(self.command.as_str())
                .args(["stop", "--time", "0", id.as_str()])
                .status();
            let _ = Command::new(self.command.as_str())
                .args(["rm", "-f", id.as_str()])
                .status();
        }
        let _ = fs::remove_file(&self.cidfile);
    }

    fn container_id(&self) -> Option<String> {
        let text = fs::read_to_string(&self.cidfile).ok()?;
        let id = text.trim();
        if id.is_empty() {
            None
        } else {
            Some(id.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PodmanContainerCleanup;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn ignores_missing_cidfile() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rack-ai-missing-{nanos}.cid"));
        let cleanup = PodmanContainerCleanup::new("podman".to_string(), path);
        cleanup.stop_and_remove();
    }

    #[test]
    fn removes_cidfile_after_cleanup() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rack-ai-empty-{nanos}.cid"));
        fs::write(&path, "\n").unwrap();
        let cleanup = PodmanContainerCleanup::new("__missing_podman__".to_string(), path.clone());
        cleanup.stop_and_remove();
        assert!(!path.exists());
    }
}
