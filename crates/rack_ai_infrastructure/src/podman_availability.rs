use std::io::ErrorKind;
use std::process::Command;

pub struct PodmanAvailability;

impl PodmanAvailability {
    pub fn ensure() -> Result<(), String> {
        match Command::new("podman").arg("--version").output() {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(if stderr.is_empty() {
                    "podman is not available; rootless Podman is required for external-repository command execution".to_string()
                } else {
                    stderr
                })
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Err(
                "podman is not available; rootless Podman is required for external-repository command execution".to_string(),
            ),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PodmanAvailability;

    #[test]
    fn reports_missing_podman_explicitly_on_this_host() {
        match PodmanAvailability::ensure() {
            Ok(()) => {}
            Err(error) => assert!(error.contains("podman is not available")),
        }
    }
}
