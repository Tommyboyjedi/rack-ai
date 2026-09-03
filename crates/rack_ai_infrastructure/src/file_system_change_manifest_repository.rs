use std::fs;

use rack_ai_application::ChangeManifestRepository;
use rack_ai_application::GenericRoutingHeader;
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
    fn has_idempotent_submission(&self, header: &GenericRoutingHeader) -> Result<bool, String> {
        let changes = self.paths.changes_dir();
        if !changes.exists() {
            return Ok(false);
        }
        for entry in fs::read_dir(changes).map_err(|error| error.to_string())? {
            let path = entry
                .map_err(|error| error.to_string())?
                .path()
                .join("review-packet.json");
            if !path.exists() {
                continue;
            }
            let packet = serde_json::from_str::<ReviewPacket>(
                &fs::read_to_string(path).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            if packet.selection_decision().is_some_and(|decision| {
                decision.source_system == header.source_system
                    && decision.work_id == header.work_id
                    && decision.submission_id == header.submission_id
                    && decision.idempotency_key == header.idempotency_key
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }

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
    use rack_ai_application::GenericCapability;
    use rack_ai_application::GenericPriority;
    use rack_ai_application::GenericRoutingHeader;
    use rack_ai_application::GenericWorkerSelectionDecision;
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

    #[test]
    fn identifies_persisted_idempotent_submission() {
        let root = temp_root();
        let repository = FileSystemChangeManifestRepository::new(RepositoryPaths::new(root));
        let header = GenericRoutingHeader::new(
            "neutral".into(),
            "work-a".into(),
            "submission-a".into(),
            "idempotency-a".into(),
            vec![GenericCapability::Coding],
            GenericPriority::Medium,
        )
        .unwrap();
        let packet = ReviewPacket::new("job-1".to_string(), "neutral".to_string())
            .with_selection_decision(GenericWorkerSelectionDecision::new(
                &header,
                rack_ai_domain::WorkUnitComplexity::Small,
                false,
            ));
        repository.save(&packet).unwrap();
        assert!(repository.has_idempotent_submission(&header).unwrap());
        let new_submission = GenericRoutingHeader::new(
            "neutral".into(),
            "work-a".into(),
            "submission-b".into(),
            "idempotency-a".into(),
            vec![GenericCapability::Coding],
            GenericPriority::Medium,
        )
        .unwrap();
        assert!(
            !repository
                .has_idempotent_submission(&new_submission)
                .unwrap()
        );
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
