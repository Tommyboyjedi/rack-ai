use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rack_ai_application::ChangeExecutionMode;
use rack_ai_application::ChangeImplementer;
use rack_ai_application::ExecuteChange;
use rack_ai_application::ExecuteChangeDependencies;
use rack_ai_application::ExecuteChangeRequest;
use rack_ai_application::ImplementChangeRequest;
use rack_ai_application::ImplementChangeResult;
use rack_ai_application::RepositoryRegistry;
use rack_ai_application::RunCommandRequest;
use rack_ai_application::WorkspaceExecutor;
use rack_ai_domain::AcceptanceVerdict;
use rack_ai_domain::ChangeStatus;

use crate::FileSystemChangeManifestRepository;
use crate::FileSystemRepositoryRegistry;
use crate::GitCommand;
use crate::GitCommandWorktree;
use crate::PodmanAvailability;
use crate::PodmanWorkspaceExecutor;
use crate::RegistryPaths;
use crate::RepositoryPaths;

const LIVE_IMAGE: &str = "docker.io/library/rust:bookworm";

struct ForbiddenBashImplementer {
    executor: PodmanWorkspaceExecutor,
}

impl ChangeImplementer for ForbiddenBashImplementer {
    fn implement(&self, request: &ImplementChangeRequest) -> Result<ImplementChangeResult, String> {
        let result = self.executor.run_command(
            &RunCommandRequest::new(
                request.worktree_path().to_path_buf(),
                vec![
                    "/bin/sh".to_string(),
                    "-lc".to_string(),
                    "printf 'pwned\\n' > README.md".to_string(),
                ],
            )?
            .with_timeout_seconds(request.timeout_seconds()),
        )?;
        if !result.evidence().succeeded() {
            return Err(format!(
                "forbidden bash write failed: {} {}",
                result.evidence().stdout(),
                result.evidence().stderr()
            ));
        }
        Ok(ImplementChangeResult::new("COMPLETE".to_string()))
    }
}

#[test]
fn live_podman_bash_forbidden_write_rejected_by_path_gate() {
    if std::env::var("RACK_AI_LIVE_PATH_POLICY_SMOKE")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    PodmanAvailability::ensure().expect("rootless podman is required for this smoke");
    PodmanAvailability::ensure_image("podman", LIVE_IMAGE)
        .expect("executor image is required for this smoke");

    let root = temp_root();
    let fixture = init_fixture(root.join("app"));
    let rack = root.join("rack");
    fs::create_dir_all(rack.join("config")).unwrap();
    fs::write(
        rack.join("config/repositories.json"),
        format!(
            r#"{{
                "workspace_root": "{}/workspaces",
                "executor": {{"image": "{LIVE_IMAGE}"}},
                "repositories": [{{"id": "fixture", "root": "{}"}}]
            }}"#,
            root.display(),
            fixture.display()
        ),
    )
    .unwrap();
    let base_sha = GitCommand::run(&fixture, &["rev-parse", "HEAD"]).unwrap();
    let document = serde_json::from_value(serde_json::json!({
        "change_id": "live-policy-001",
        "repository": {
            "id": "fixture",
            "registered_root": fixture.to_string_lossy(),
            "base_ref": "main",
            "base_sha": base_sha
        },
        "task": "Write a forbidden file.",
        "allowed_paths": ["src/"],
        "acceptance": {"commands": [["cargo", "test"]]},
        "limits": {"max_implementation_attempts": 1, "timeout_seconds": 60, "network": "disabled"}
    }))
    .unwrap();

    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(rack.clone()));
    let git = GitCommandWorktree;
    let manifests = FileSystemChangeManifestRepository::new(RepositoryPaths::new(rack.clone()));
    let policy = registry.command_policy().unwrap();
    let executor = PodmanWorkspaceExecutor::new(registry.executor_config().unwrap());
    let implementer = ForbiddenBashImplementer {
        executor: PodmanWorkspaceExecutor::new(registry.executor_config().unwrap()),
    };
    let result = ExecuteChange::new(ExecuteChangeDependencies {
        registry: &registry,
        command_policy: &policy,
        git: &git,
        manifests: &manifests,
        executor: Some(&executor),
        implementer: Some(&implementer),
    })
    .execute(ExecuteChangeRequest {
        document,
        mode: ChangeExecutionMode::ImplementAndVerify,
    })
    .unwrap();

    assert_eq!(result.packet.status(), &ChangeStatus::PathPolicyFailed);
    assert_eq!(
        result.packet.acceptance_verdict(),
        Some(&AcceptanceVerdict::Rejected)
    );
    assert!(result.packet.last_error().unwrap().contains("README.md"));
    assert!(result.packet.commands().is_empty());
    let worktree = root.join("workspaces/live-policy-001/repo");
    assert_eq!(
        fs::read_to_string(worktree.join("README.md"))
            .unwrap()
            .trim(),
        "pwned"
    );
    assert_eq!(
        GitCommand::run(&fixture, &["rev-parse", "HEAD"]).unwrap(),
        base_sha
    );
    assert_eq!(
        fs::read_to_string(fixture.join("README.md"))
            .unwrap()
            .trim(),
        "safe"
    );
}

fn init_fixture(root: PathBuf) -> PathBuf {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    fs::write(root.join("README.md"), "safe\n").unwrap();
    let status = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());
    GitCommand::run(&root, &["config", "user.email", "test@example.com"]).unwrap();
    GitCommand::run(&root, &["config", "user.name", "test"]).unwrap();
    GitCommand::run(&root, &["add", "."]).unwrap();
    GitCommand::run(&root, &["commit", "-m", "init"]).unwrap();
    root
}

fn temp_root() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rack-ai-live-policy-{nanos}"));
    fs::create_dir_all(&root).unwrap();
    root
}
