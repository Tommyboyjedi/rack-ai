use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rack_ai_application::CreateChangeWorktreeRequest;
use rack_ai_application::GitWorktree;
use rack_ai_application::InspectChangeWorktreeRequest;
use rack_ai_application::RepositoryRegistry;
use rack_ai_application::ResolveGitShaRequest;
use rack_ai_domain::GitRef;
use serde_json::Value;

use crate::FileSystemRepositoryRegistry;
use crate::GitCommandWorktree;
use crate::RegistryPaths;

#[test]
fn nested_cargo_workspace_root_is_externalized_and_target_cargo_runs_in_target_context() {
    let root = temp_root();
    let live = init_live_rack_repo(&root.join("live-rack-ai"));
    let target = init_target_repo(&root.join("tiny-ticket"));
    write_repositories_document(&live, &target, &live.join("state/workspaces"));

    let registry = FileSystemRepositoryRegistry::new(RegistryPaths::new(live.clone()));
    let workspace_root = registry.workspace_root().unwrap();
    let live_repo = fs::canonicalize(&live).unwrap();
    let workspaces = workspace_root.as_path().to_path_buf();
    assert!(!workspaces.starts_with(&live_repo));

    let git = GitCommandWorktree;
    let base_sha = git
        .resolve_sha(&ResolveGitShaRequest::new(
            target.clone(),
            GitRef::new("main".to_string()).unwrap(),
        ))
        .unwrap();
    let worktree = workspaces.join("tiny-ticket-001/repo");
    git.create(
        &CreateChangeWorktreeRequest::new(target.clone(), base_sha.clone())
            .with_branch_name("rack/change-tiny-ticket-001".to_string())
            .with_worktree_path(worktree.clone()),
    )
    .unwrap();

    let evidence = git
        .inspect(&InspectChangeWorktreeRequest::new(
            worktree.clone(),
            base_sha,
        ))
        .unwrap();
    assert!(evidence.changed_paths().is_empty());
    assert_eq!(
        GitCommandWorktree.current_branch(&worktree).unwrap(),
        "rack/change-tiny-ticket-001"
    );

    let metadata = run_cargo_json(
        &worktree,
        &["metadata", "--format-version", "1", "--no-deps"],
    );
    let workspace = metadata["workspace_root"].as_str().unwrap();
    assert_eq!(PathBuf::from(workspace), worktree);

    run_cargo(&worktree, &["test"]);
}

fn run_cargo_json(worktree: &Path, args: &[&str]) -> Value {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(worktree)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_cargo(worktree: &Path, args: &[&str]) {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(worktree)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_repositories_document(live_root: &Path, target_root: &Path, workspace_root: &Path) {
    fs::create_dir_all(live_root.join("config")).unwrap();
    fs::write(
        live_root.join("config/repositories.json"),
        format!(
            concat!(
                "{{",
                "\"workspace_root\":\"{}\",",
                "\"executor\":{{\"backend\":\"podman\",\"image\":\"rust:bookworm\"}},",
                "\"repositories\":[{{",
                "\"id\":\"target\",",
                "\"root\":\"{}\",",
                "\"default_base_ref\":\"main\",",
                "\"enabled\":true",
                "}}]",
                "}}"
            ),
            workspace_root.display(),
            target_root.display()
        ),
    )
    .unwrap();
}

fn init_live_rack_repo(path: &Path) -> PathBuf {
    fs::create_dir_all(path.join("crates/example/src")).unwrap();
    fs::write(
        path.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        path.join("crates/example/Cargo.toml"),
        live_member_manifest(),
    )
    .unwrap();
    fs::write(
        path.join("crates/example/src/lib.rs"),
        "pub fn marker() -> u32 { 1 }\n",
    )
    .unwrap();
    init_git_repo(path)
}

fn init_target_repo(path: &Path) -> PathBuf {
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(path.join("Cargo.toml"), target_manifest()).unwrap();
    fs::write(path.join("src/lib.rs"), target_source()).unwrap();
    init_git_repo(path)
}

fn init_git_repo(path: &Path) -> PathBuf {
    let status = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
    let status = Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
    let status = Command::new("git")
        .args(["config", "user.name", "test"])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
    let status = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
    let status = Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
    path.to_path_buf()
}

fn live_member_manifest() -> &'static str {
    "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
}

fn target_manifest() -> &'static str {
    "[package]\nname = \"tiny-ticket\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
}

fn target_source() -> &'static str {
    "#[cfg(test)]\nmod tests {\n    #[test]\n    fn target_repo_runs_in_its_own_workspace() {\n        assert_eq!(2 + 2, 4);\n    }\n}\n"
}

fn temp_root() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rack-ai-workspace-isolation-{nanos}"));
    fs::create_dir_all(&root).unwrap();
    root
}
