use std::fs;

use rack_ai_application::ChangeManifestRepository;
use rack_ai_application::ReviewPacket;

use crate::RepositoryPaths;

pub struct FileSystemChangeManifestRepository {
    paths: RepositoryPaths,
}

impl FileSystemChangeManifestRepository {
    pub fn new(paths: RepositoryPaths) -> Self {
        Self { paths }
    }
}

impl ChangeManifestRepository for FileSystemChangeManifestRepository {
    fn save(&self, packet: &ReviewPacket) -> Result<String, String> {
        let dir = self.paths.changes_dir().join(packet.change_id());
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let path = dir.join("review-packet.json");
        let json = serde_json::to_string_pretty(packet).map_err(|error| error.to_string())?;
        fs::write(&path, format!("{json}\n")).map_err(|error| error.to_string())?;
        Ok(path.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::FileSystemChangeManifestRepository;
    use crate::RepositoryPaths;
    use rack_ai_application::ChangeManifestRepository;
    use rack_ai_application::ReviewPacket;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_review_packet_under_change_id() {
        let root = temp_root();
        let repository =
            FileSystemChangeManifestRepository::new(RepositoryPaths::new(root.clone()));
        let path = repository
            .save(&ReviewPacket::new(
                "job-1".to_string(),
                "adaptos".to_string(),
            ))
            .unwrap();
        assert!(path.ends_with("state/changes/job-1/review-packet.json"));
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("job-1"));
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-manifest-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
