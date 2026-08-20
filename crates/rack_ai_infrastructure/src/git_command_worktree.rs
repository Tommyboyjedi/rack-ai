use std::fs;

use rack_ai_application::ChangeWorkspace;
use rack_ai_application::CreateChangeWorktreeRequest;
use rack_ai_application::GitEvidence;
use rack_ai_application::GitWorktree;
use rack_ai_application::InspectChangeWorktreeRequest;
use rack_ai_application::ResolveGitShaRequest;
use rack_ai_domain::ChangeId;
use rack_ai_domain::GitSha;

use crate::ChangedPaths;
use crate::GitCommand;

pub struct GitCommandWorktree;

impl GitWorktree for GitCommandWorktree {
    fn resolve_sha(&self, request: &ResolveGitShaRequest) -> Result<GitSha, String> {
        GitSha::new(GitCommand::run(
            request.repository_root(),
            &["rev-parse", request.git_ref().value()],
        )?)
    }

    fn create(&self, request: &CreateChangeWorktreeRequest) -> Result<ChangeWorkspace, String> {
        if request.worktree_path().exists() {
            return Err(format!(
                "worktree already exists: {}",
                request.worktree_path().display()
            ));
        }
        if let Some(parent) = request.worktree_path().parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let worktree_path = request.worktree_path().to_string_lossy().into_owned();
        GitCommand::run(
            request.repository_root(),
            &[
                "worktree",
                "add",
                "-b",
                request.branch_name(),
                worktree_path.as_str(),
                request.base_sha().value(),
            ],
        )?;
        let change_id = request
            .branch_name()
            .strip_prefix("rack/change-")
            .unwrap_or(request.branch_name());
        Ok(ChangeWorkspace::new(
            ChangeId::new(change_id.to_string())?,
            request.worktree_path().to_path_buf(),
        )
        .with_branch_name(request.branch_name().to_string())
        .with_base_sha(request.base_sha().clone()))
    }

    fn inspect(&self, request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String> {
        if !request.worktree_path().exists() {
            return Err(format!(
                "worktree does not exist: {}",
                request.worktree_path().display()
            ));
        }
        let head = GitSha::new(GitCommand::run(
            request.worktree_path(),
            &["rev-parse", "HEAD"],
        )?)?;
        if &head != request.expected_base_sha() {
            return Err("worktree is not at the recorded base sha".to_string());
        }
        let status = GitCommand::run(request.worktree_path(), &["status", "--porcelain"])?;
        let diff = GitCommand::run(request.worktree_path(), &["diff"])?;
        let diff_stat = GitCommand::run(request.worktree_path(), &["diff", "--stat"])?;
        Ok(GitEvidence::new(head, status.clone())
            .with_diff(diff)
            .with_diff_stat(diff_stat)
            .with_changed_paths(ChangedPaths::from_porcelain(&status)))
    }
}

#[cfg(test)]
mod tests {
    use super::GitCommandWorktree;
    use crate::GitCommand;
    use rack_ai_application::CreateChangeWorktreeRequest;
    use rack_ai_application::GitWorktree;
    use rack_ai_application::InspectChangeWorktreeRequest;
    use rack_ai_application::ResolveGitShaRequest;
    use rack_ai_domain::GitRef;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn creates_isolated_worktree_from_recorded_sha() {
        let fixture = init_fixture();
        let git = GitCommandWorktree;
        let sha = git
            .resolve_sha(&ResolveGitShaRequest::new(
                fixture.clone(),
                GitRef::new("main".to_string()).unwrap(),
            ))
            .unwrap();
        let worktree = fixture.parent().unwrap().join(format!(
            "change-worktree-{}/repo",
            fixture.file_name().unwrap().to_string_lossy()
        ));
        let workspace = git
            .create(
                &CreateChangeWorktreeRequest::new(fixture.clone(), sha.clone())
                    .with_branch_name("rack/change-job-1".to_string())
                    .with_worktree_path(worktree.clone()),
            )
            .unwrap();
        let evidence = git
            .inspect(&InspectChangeWorktreeRequest::new(
                worktree.clone(),
                sha.clone(),
            ))
            .unwrap();
        assert_eq!(evidence.head_sha(), &sha);
        assert!(evidence.changed_paths().is_empty());
        fs::write(worktree.join("src/lib.rs"), "changed\n").unwrap();
        let dirty = git
            .inspect(&InspectChangeWorktreeRequest::new(
                worktree.clone(),
                sha.clone(),
            ))
            .unwrap();
        assert_eq!(dirty.changed_paths(), ["src/lib.rs"]);
        fs::write(worktree.join("README.md"), "pwned\n").unwrap();
        let escaped = git
            .inspect(&InspectChangeWorktreeRequest::new(
                worktree.clone(),
                sha.clone(),
            ))
            .unwrap();
        let allowed = rack_ai_domain::AllowedPaths::new(vec![
            rack_ai_domain::AllowedPath::new("src".to_string()).unwrap(),
        ])
        .unwrap();
        let rejected = allowed.reject_disallowed(escaped.changed_paths());
        assert!(rejected.iter().any(|path| path.as_str() == "README.md"));
        let main_file = fs::read_to_string(fixture.join("src/lib.rs")).unwrap();
        assert_eq!(main_file, "original\n");
        assert_eq!(workspace.branch_name(), "rack/change-job-1");
    }

    fn init_fixture() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-git-fixture-{nanos}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "original\n").unwrap();
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
}
