use std::io::ErrorKind;
use std::process::Command;

use crate::RootlessFlag;

pub struct PodmanAvailability;

impl PodmanAvailability {
    pub fn ensure() -> Result<(), String> {
        Self::ensure_command("podman")
    }

    pub fn ensure_command(command: &str) -> Result<(), String> {
        match Command::new(command).arg("--version").output() {
            Ok(output) if output.status.success() => Self::ensure_rootless(command),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(if stderr.is_empty() {
                    unavailable_message()
                } else {
                    stderr
                })
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Err(unavailable_message()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn ensure_rootless(command: &str) -> Result<(), String> {
        let output = Command::new(command)
            .args(["info", "--format", "{{.Host.Security.Rootless}}"])
            .output()
            .map_err(|error| {
                if error.kind() == ErrorKind::NotFound {
                    unavailable_message()
                } else {
                    error.to_string()
                }
            })?;
        if !output.status.success() {
            return Err(unavailable_message());
        }
        RootlessFlag::assert_enabled(&String::from_utf8_lossy(&output.stdout))
    }

    pub fn ensure_image(command: &str, image: &str) -> Result<(), String> {
        let status = Command::new(command)
            .args(["image", "exists", image])
            .status()
            .map_err(|error| {
                if error.kind() == ErrorKind::NotFound {
                    unavailable_message()
                } else {
                    error.to_string()
                }
            })?;
        if status.success() {
            return Ok(());
        }
        Err(format!(
            "executor image is not present locally: {image}; pull it on the host before running change jobs"
        ))
    }
}

fn unavailable_message() -> String {
    "podman is not available; rootless Podman is required for external-repository command execution"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::PodmanAvailability;
    use crate::RootlessFlag;

    #[test]
    fn reports_missing_podman_explicitly() {
        let error =
            PodmanAvailability::ensure_command("__definitely_missing_podman_binary__").unwrap_err();
        assert!(error.contains("podman is not available"));
    }

    #[test]
    fn rootless_flag_rejects_rootful() {
        assert!(RootlessFlag::assert_enabled("false").is_err());
    }
}
