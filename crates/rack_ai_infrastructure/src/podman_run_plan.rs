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
        let build = "/rack-build";
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
            "--tmpfs".to_string(),
            format!("{build}:rw,exec,nosuid,size=512m"),
            "--memory".to_string(),
            invocation.memory().to_string(),
            "--pids-limit".to_string(),
            invocation.pids_limit().to_string(),
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
            format!("HOME={build}"),
            "--env".to_string(),
            format!("TMPDIR={build}"),
            "--env".to_string(),
            "CARGO_HOME=/usr/local/cargo".to_string(),
            "--env".to_string(),
            format!("CARGO_TARGET_DIR={build}/target"),
            "--env".to_string(),
            "RUSTUP_HOME=/usr/local/rustup".to_string(),
            "--env".to_string(),
            "LANG=C.UTF-8".to_string(),
        ];
        for resource in invocation.environment_resources() {
            arguments.push("--mount".to_string());
            arguments.push(format!(
                "type=bind,src={},dst={},ro",
                resource.source_path().display(),
                resource.container_path().display()
            ));
        }
        if let Some(cidfile) = invocation.cidfile() {
            arguments.push("--cidfile".to_string());
            arguments.push(cidfile.display().to_string());
        }
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
    use rack_ai_application::EnvironmentResourceMount;
    use std::path::PathBuf;

    #[test]
    fn isolates_network_capabilities_and_mounts() {
        let plan = PodmanRunPlan::from_invocation(
            &PodmanInvocation::new(
                "docker.io/library/rust:bookworm".to_string(),
                PathBuf::from("/tmp/work"),
            )
            .unwrap()
            .with_cidfile(PathBuf::from("/tmp/work.cid"))
            .with_environment_resources(vec![
                EnvironmentResourceMount::same_path(PathBuf::from("/srv/runtime/.venv")).unwrap(),
            ])
            .with_argv(vec!["cargo".to_string(), "test".to_string()]),
        )
        .unwrap();
        assert!(plan.contains("--network"));
        assert!(plan.contains("none"));
        assert!(plan.contains("--cap-drop"));
        assert!(plan.contains("ALL"));
        assert!(plan.contains("no-new-privileges"));
        assert!(plan.contains("type=bind,src=/tmp/work,dst=/workspace"));
        assert!(plan.contains("type=bind,src=/srv/runtime/.venv,dst=/srv/runtime/.venv,ro"));
        assert!(plan.contains("--cidfile"));
        assert!(plan.contains("/tmp/work.cid"));
        assert!(plan.contains("HOME=/rack-build"));
        assert!(plan.contains("TMPDIR=/rack-build"));
        assert!(plan.contains("CARGO_HOME=/usr/local/cargo"));
        assert!(plan.contains("CARGO_TARGET_DIR=/rack-build/target"));
        assert!(!plan.contains("/workspace/.rack-cargo"));
        assert!(!plan.contains("/workspace/target"));
        assert!(!plan.contains("docker.sock"));
        assert!(!plan.contains("src=/home"));
        assert!(!plan.contains("--privileged"));
        assert!(!plan.contains("SSH"));
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
