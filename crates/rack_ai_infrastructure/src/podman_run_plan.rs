use crate::PodmanInvocation;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PodmanRunPlan {
    arguments: Vec<String>,
}

impl PodmanRunPlan {
    pub fn from_invocation(invocation: &PodmanInvocation) -> Result<Self, String> {
        if invocation.argv().is_empty() {
            return Err("podman invocation command cannot be empty".to_string());
        }
        let mut arguments = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--network".to_string(),
            "none".to_string(),
            "--cap-drop".to_string(),
            "ALL".to_string(),
            "--security-opt".to_string(),
            "no-new-privileges".to_string(),
            "--read-only".to_string(),
            "--tmpfs".to_string(),
            "/tmp:rw,noexec,nosuid,size=64m".to_string(),
            "--memory".to_string(),
            invocation.memory().to_string(),
            "--pids-limit".to_string(),
            invocation.pids_limit().to_string(),
            "--timeout".to_string(),
            invocation.timeout_seconds().to_string(),
            "--userns".to_string(),
            "keep-id".to_string(),
            "--mount".to_string(),
            format!(
                "type=bind,src={},dst={}",
                invocation.worktree_path().display(),
                invocation.workspace_mount()
            ),
            "--workdir".to_string(),
            invocation.workspace_mount().to_string(),
            "--env".to_string(),
            "PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/bin"
                .to_string(),
            "--env".to_string(),
            format!("HOME={}", invocation.workspace_mount()),
            "--env".to_string(),
            format!("CARGO_HOME={}/.rack-cargo", invocation.workspace_mount()),
            "--env".to_string(),
            format!("CARGO_TARGET_DIR={}/target", invocation.workspace_mount()),
            "--env".to_string(),
            "RUSTUP_HOME=/usr/local/rustup".to_string(),
            "--env".to_string(),
            "LANG=C.UTF-8".to_string(),
        ];
        if invocation.stdin().is_some() {
            arguments.insert(1, "-i".to_string());
        }
        arguments.push(invocation.image().to_string());
        arguments.extend(invocation.argv().iter().cloned());
        Ok(Self { arguments })
    }

    pub fn arguments(&self) -> &[String] {
        self.arguments.as_slice()
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.arguments.iter().any(|item| item.contains(needle))
    }
}

#[cfg(test)]
mod tests {
    use super::PodmanRunPlan;
    use crate::PodmanInvocation;
    use std::path::PathBuf;

    #[test]
    fn isolates_network_capabilities_and_mounts() {
        let plan = PodmanRunPlan::from_invocation(
            &PodmanInvocation::new(
                "docker.io/library/rust:bookworm".to_string(),
                PathBuf::from("/tmp/work"),
            )
            .unwrap()
            .with_argv(vec!["cargo".to_string(), "test".to_string()]),
        )
        .unwrap();
        assert!(plan.contains("--network"));
        assert!(plan.contains("none"));
        assert!(plan.contains("--cap-drop"));
        assert!(plan.contains("ALL"));
        assert!(plan.contains("no-new-privileges"));
        assert!(plan.contains("type=bind,src=/tmp/work,dst=/workspace"));
        assert!(!plan.contains("docker.sock"));
        assert!(!plan.contains("/home"));
        assert!(!plan.contains("--privileged"));
        assert!(!plan.contains("SSH"));
        assert!(plan.contains("CARGO_HOME=/workspace/.rack-cargo"));
        assert_eq!(plan.arguments().last(), Some(&"test".to_string()));
    }

    #[test]
    fn adds_interactive_flag_when_stdin_present() {
        let plan = PodmanRunPlan::from_invocation(
            &PodmanInvocation::new("rust:bookworm".to_string(), PathBuf::from("/tmp/work"))
                .unwrap()
                .with_argv(vec!["cat".to_string()])
                .with_stdin(Some("hello".to_string())),
        )
        .unwrap();
        assert_eq!(plan.arguments()[1], "-i");
    }
}
