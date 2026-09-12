use std::fs;
use std::path::Path;

use rack_ai_application::CampaignCommitRequest;
use rack_ai_application::ChangeWorkspace;
use rack_ai_application::CreateChangeWorktreeRequest;
use rack_ai_application::GitEvidence;
use rack_ai_application::GitWorktree;
use rack_ai_application::InspectChangeWorktreeRequest;
use rack_ai_application::ResolveGitShaRequest;
use rack_ai_application::assert_campaign_git_args;
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
        let change_id = branch_change_id(request.branch_name());
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
        let status = GitCommand::run(
            request.worktree_path(),
            &["status", "--porcelain=v1", "-uall"],
        )?;
        let diff = GitCommand::run(request.worktree_path(), &["diff"])?;
        let diff_stat = GitCommand::run(request.worktree_path(), &["diff", "--stat"])?;
        Ok(GitEvidence::new(head, status.clone())
            .with_diff(diff)
            .with_diff_stat(diff_stat)
            .with_changed_paths(ChangedPaths::from_porcelain(&status)))
    }

    fn snapshot(&self, worktree_path: &Path) -> Result<GitEvidence, String> {
        if !worktree_path.exists() {
            return Err(format!(
                "worktree does not exist: {}",
                worktree_path.display()
            ));
        }
        run_campaign_git(worktree_path, &["rev-parse", "HEAD"])?;
        let head = GitSha::new(run_campaign_git(worktree_path, &["rev-parse", "HEAD"])?)?;
        let status = run_campaign_git(worktree_path, &["status", "--porcelain=v1", "-uall"])?;
        let diff = run_campaign_git(worktree_path, &["diff"])?;
        let diff_stat = run_campaign_git(worktree_path, &["diff", "--stat"])?;
        Ok(GitEvidence::new(head, status.clone())
            .with_diff(diff)
            .with_diff_stat(diff_stat)
            .with_changed_paths(ChangedPaths::from_porcelain(&status)))
    }

    fn current_branch(&self, worktree_path: &Path) -> Result<String, String> {
        run_campaign_git(worktree_path, &["branch", "--show-current"])
    }

    fn current_head(&self, worktree_path: &Path) -> Result<GitSha, String> {
        GitSha::new(run_campaign_git(worktree_path, &["rev-parse", "HEAD"])?)
    }

    fn commit_local(&self, request: &CampaignCommitRequest) -> Result<GitSha, String> {
        if request.paths().is_empty() {
            return Err("cannot commit an empty source diff".to_string());
        }
        let mut add_args = vec!["add", "--"];
        let path_refs: Vec<&str> = request.paths().iter().map(|path| path.as_str()).collect();
        add_args.extend(path_refs.iter().copied());
        run_campaign_git(request.worktree_path(), &add_args)?;
        let name = format!("user.name={}", request.author_name());
        let email = format!("user.email={}", request.author_email());
        run_campaign_git(
            request.worktree_path(),
            &[
                "-c",
                name.as_str(),
                "-c",
                email.as_str(),
                "commit",
                "-m",
                request.message(),
                "--",
            ]
            .into_iter()
            .chain(path_refs.iter().copied())
            .collect::<Vec<_>>()
            .as_slice(),
        )?;
        GitSha::new(run_campaign_git(
            request.worktree_path(),
            &["rev-parse", "HEAD"],
        )?)
    }

    fn reset_managed_worktree(
        &self,
        worktree_path: &Path,
        expected_head: &GitSha,
        dirty_paths: &[String],
    ) -> Result<(), String> {
        let actual_head = GitSha::new(run_campaign_git(worktree_path, &["rev-parse", "HEAD"])?)?;
        if &actual_head != expected_head {
            return Err("worktree HEAD changed before managed reset".to_string());
        }
        for relative in dirty_paths {
            let path = managed_dirty_path(worktree_path, relative)?;
            if path.is_dir() {
                fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
            } else if path.exists() {
                fs::remove_file(&path).map_err(|error| error.to_string())?;
            }
        }
        GitCommand::run(worktree_path, &["reset", "--hard", expected_head.value()])?;
        Ok(())
    }
}

fn run_campaign_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    assert_campaign_git_args(args)?;
    GitCommand::run(repo, args)
}

fn managed_dirty_path(worktree_path: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    let path = worktree_path.join(relative);
    if !path.starts_with(worktree_path) {
        return Err(format!("dirty path escapes managed worktree: {relative}"));
    }
    Ok(path)
}

fn branch_change_id(branch_name: &str) -> &str {
    branch_name
        .strip_prefix("rack/change-")
        .or_else(|| branch_name.strip_prefix("rack/campaign-"))
        .unwrap_or(branch_name)
}

#[cfg(test)]
mod tests {
    use super::GitCommandWorktree;
    use super::branch_change_id;
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

    #[test]
    fn keeps_campaign_branch_ids_filesystem_safe() {
        assert_eq!(
            branch_change_id("rack/campaign-adaptos-foundation-20260821"),
            "adaptos-foundation-20260821"
        );
    }

    #[test]
    fn reports_untracked_file_exactly_and_preserves_single_file_path_policy() {
        let fixture = init_fixture();
        let git = GitCommandWorktree;
        let sha = git
            .resolve_sha(&ResolveGitShaRequest::new(
                fixture.clone(),
                GitRef::new("main".to_string()).unwrap(),
            ))
            .unwrap();
        let worktree = fixture.parent().unwrap().join(format!(
            "change-worktree-single-file-{}/repo",
            fixture.file_name().unwrap().to_string_lossy()
        ));
        git.create(
            &CreateChangeWorktreeRequest::new(fixture.clone(), sha.clone())
                .with_branch_name("rack/change-job-single-file".to_string())
                .with_worktree_path(worktree.clone()),
        )
        .unwrap();
        let test_file = worktree.join("tests/test_reservation_book.py");
        fs::create_dir_all(test_file.parent().unwrap()).unwrap();
        fs::write(
            &test_file,
            "def test_placeholder():
    assert True
",
        )
        .unwrap();
        let untracked = git
            .inspect(&InspectChangeWorktreeRequest::new(
                worktree.clone(),
                sha.clone(),
            ))
            .unwrap();
        assert_eq!(
            untracked.changed_paths(),
            ["tests/test_reservation_book.py"]
        );
        let exact_allow = rack_ai_domain::AllowedPaths::new(vec![
            rack_ai_domain::AllowedPath::new("tests/test_reservation_book.py".to_string()).unwrap(),
        ])
        .unwrap();
        assert!(
            exact_allow
                .reject_disallowed(untracked.changed_paths())
                .is_empty()
        );
        fs::write(
            worktree.join("tests/helper.py"),
            "def helper():
    return 1
",
        )
        .unwrap();
        let with_sibling = git
            .inspect(&InspectChangeWorktreeRequest::new(worktree, sha))
            .unwrap();
        let rejected = exact_allow.reject_disallowed(with_sibling.changed_paths());
        assert!(
            rejected
                .iter()
                .any(|path| path.as_str() == "tests/helper.py")
        );
    }

    #[test]
    fn reports_multiple_untracked_files_exactly() {
        let fixture = init_fixture();
        let git = GitCommandWorktree;
        let sha = git
            .resolve_sha(&ResolveGitShaRequest::new(
                fixture.clone(),
                GitRef::new("main".to_string()).unwrap(),
            ))
            .unwrap();
        let worktree = fixture.parent().unwrap().join(format!(
            "change-worktree-untracked-{}/repo",
            fixture.file_name().unwrap().to_string_lossy()
        ));
        git.create(
            &CreateChangeWorktreeRequest::new(fixture.clone(), sha.clone())
                .with_branch_name("rack/change-job-untracked".to_string())
                .with_worktree_path(worktree.clone()),
        )
        .unwrap();
        fs::create_dir_all(worktree.join("tests/subdir")).unwrap();
        fs::write(
            worktree.join("tests/test_reservation_book.py"),
            "def test_one():
    assert True
",
        )
        .unwrap();
        fs::write(
            worktree.join("tests/subdir/test_other.py"),
            "def test_two():
    assert True
",
        )
        .unwrap();
        let dirty = git
            .inspect(&InspectChangeWorktreeRequest::new(worktree, sha))
            .unwrap();
        assert!(
            dirty
                .changed_paths()
                .iter()
                .any(|path| path == "tests/test_reservation_book.py")
        );
        assert!(
            dirty
                .changed_paths()
                .iter()
                .any(|path| path == "tests/subdir/test_other.py")
        );
        assert!(!dirty.changed_paths().iter().any(|path| path == "tests/"));
    }

    #[test]
    fn reports_deleted_files_exactly() {
        let fixture = init_fixture();
        let git = GitCommandWorktree;
        let sha = git
            .resolve_sha(&ResolveGitShaRequest::new(
                fixture.clone(),
                GitRef::new("main".to_string()).unwrap(),
            ))
            .unwrap();
        let worktree = fixture.parent().unwrap().join(format!(
            "change-worktree-delete-{}/repo",
            fixture.file_name().unwrap().to_string_lossy()
        ));
        git.create(
            &CreateChangeWorktreeRequest::new(fixture.clone(), sha.clone())
                .with_branch_name("rack/change-job-delete".to_string())
                .with_worktree_path(worktree.clone()),
        )
        .unwrap();
        fs::remove_file(worktree.join("src/lib.rs")).unwrap();
        let dirty = git
            .inspect(&InspectChangeWorktreeRequest::new(worktree, sha))
            .unwrap();
        assert_eq!(dirty.changed_paths(), ["src/lib.rs"]);
    }

    #[test]
    fn commit_local_creates_commit_with_selected_content() {
        let fixture = init_fixture();
        let git = GitCommandWorktree;
        let sha = git
            .resolve_sha(&ResolveGitShaRequest::new(
                fixture.clone(),
                GitRef::new("main".to_string()).unwrap(),
            ))
            .unwrap();
        let worktree = fixture.parent().unwrap().join(format!(
            "change-worktree-commit-{}/repo",
            fixture.file_name().unwrap().to_string_lossy()
        ));
        git.create(
            &CreateChangeWorktreeRequest::new(fixture.clone(), sha.clone())
                .with_branch_name("rack/change-job-commit".to_string())
                .with_worktree_path(worktree.clone()),
        )
        .unwrap();
        fs::write(worktree.join("src/lib.rs"), "committed\n").unwrap();
        let commit = git
            .commit_local(&rack_ai_application::CampaignCommitRequest::new(
                worktree.clone(),
                "job-commit",
                "accepted-change",
                vec!["src/lib.rs".to_string()],
            ))
            .unwrap();
        assert_ne!(commit, sha);
        let shown = GitCommand::run(
            &worktree,
            &["show", &format!("{}:src/lib.rs", commit.value())],
        )
        .unwrap();
        assert_eq!(shown, "committed");
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
