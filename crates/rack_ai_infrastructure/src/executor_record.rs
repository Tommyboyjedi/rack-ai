use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExecutorRecord {
    #[serde(default = "podman_backend")]
    pub backend: String,
    #[serde(default)]
    pub image: String,
    #[serde(default = "workspace_mount")]
    pub workspace_path: String,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default = "default_pids")]
    pub pids_limit: u32,
}

fn podman_backend() -> String {
    "podman".to_string()
}

fn workspace_mount() -> String {
    "/workspace".to_string()
}

fn default_memory() -> String {
    "2g".to_string()
}

fn default_pids() -> u32 {
    256
}

#[cfg(test)]
mod tests {
    use super::ExecutorRecord;

    #[test]
    fn defaults_podman_workspace_mount() {
        let record = serde_json::from_str::<ExecutorRecord>(
            "{\"image\":\"docker.io/library/rust:bookworm\"}",
        )
        .unwrap();
        assert_eq!(record.backend, "podman");
        assert_eq!(record.workspace_path, "/workspace");
    }

    #[test]
    fn parses_host_backend_without_image() {
        let record = serde_json::from_str::<ExecutorRecord>("{\"backend\":\"host\"}").unwrap();
        assert_eq!(record.backend, "host");
        assert_eq!(record.image, "");
    }
}
