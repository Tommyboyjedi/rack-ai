//! A positive socket link proves ownership; unrelated disappearing descriptors do not.
pub const SCAN: &str = r#"for fd in "$1"/[0-9]*/fd/*; do readlink "$fd" 2>/dev/null || :; done"#;

pub fn require_listener(descriptors: &str, inode: &str) -> Result<(), String> {
    if descriptors.lines().any(|line| line == inode) {
        Ok(())
    } else {
        Err("endpoint_not_owned_by_container".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink, process::Command};

    #[test]
    fn disappearing_process_does_not_hide_positive_socket_proof() {
        let root = std::env::temp_dir().join(format!(
            "rack-socket-probe-{}",
            crate::types::identity().unwrap()
        ));
        fs::create_dir_all(root.join("000/fd")).unwrap();
        fs::create_dir_all(root.join("001/fd")).unwrap();
        symlink("socket:[4242]", root.join("000/fd/3")).unwrap();
        symlink("socket:[9999]", root.join("001/fd/4")).unwrap();
        // Simulate a process exiting after expansion of the scan paths.
        let prefix = r#"readlink() {
            /usr/bin/readlink "$1"; result=$?
            /bin/rm -rf -- "$RACK_TEST_GONE"
            return "$result"
        }; "#;
        let run = |scan: &str| {
            Command::new("timeout")
                .args(["5", "/bin/sh", "-c", &format!("{prefix}{scan}"), "probe"])
                .arg(&root)
                .env("RACK_TEST_GONE", root.join("001"))
                .output()
                .unwrap()
        };
        let observed = run(SCAN);
        assert!(observed.status.success());
        let links = String::from_utf8(observed.stdout).unwrap();
        assert!(require_listener(&links, "socket:[4242]").is_ok());
        assert!(require_listener(&links, "socket:[9999]").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn absent_or_unreadable_evidence_never_proves_ownership() {
        assert!(require_listener("", "socket:[4242]").is_err());
        assert!(require_listener("socket:[9999]\n", "socket:[4242]").is_err());
        assert!(require_listener("/tmp/socket:[4242]\n", "socket:[4242]").is_err());
    }
}
