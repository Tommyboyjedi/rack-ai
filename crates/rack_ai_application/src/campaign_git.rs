use std::path::Path;
use std::path::PathBuf;

use rack_ai_domain::GitSha;

use crate::ChangeWorkspace;
use crate::CreateChangeWorktreeRequest;
use crate::GitEvidence;
use crate::GitWorktree;
use crate::InspectChangeWorktreeRequest;
use crate::ResolveGitShaRequest;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignCommitRequest {
    worktree_path: PathBuf,
    message: String,
    paths: Vec<String>,
    author_name: String,
    author_email: String,
}

impl CampaignCommitRequest {
    pub fn new(
        worktree_path: PathBuf,
        campaign_id: &str,
        step_id: &str,
        paths: Vec<String>,
    ) -> Self {
        Self {
            worktree_path,
            message: format!("rack({campaign_id}): {step_id}"),
            paths,
            author_name: "Rack AI Campaign".to_string(),
            author_email: "rack-ai-campaign@local".to_string(),
        }
    }

    pub fn worktree_path(&self) -> &Path {
        self.worktree_path.as_path()
    }

    pub fn message(&self) -> &str {
        self.message.as_str()
    }

    pub fn paths(&self) -> &[String] {
        self.paths.as_slice()
    }

    pub fn author_name(&self) -> &str {
        self.author_name.as_str()
    }

    pub fn author_email(&self) -> &str {
        self.author_email.as_str()
    }
}

const FORBIDDEN_GIT_COMMANDS: &[&str] = &[
    "push",
    "fetch",
    "pull",
    "remote",
    "merge",
    "rebase",
    "reset",
    "clean",
    "tag",
    "checkout",
    "config",
    "gc",
    "filter-branch",
    "update-ref",
];

pub fn assert_campaign_git_args(args: &[&str]) -> Result<(), String> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        if arg == "-c" {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        if FORBIDDEN_GIT_COMMANDS.contains(&arg) {
            return Err(format!("forbidden campaign git operation: {arg}"));
        }
        return Ok(());
    }
    Err("campaign git command is missing".to_string())
}

pub trait CampaignGit: GitWorktree {
    fn snapshot(&self, worktree_path: &Path) -> Result<GitEvidence, String>;
    fn current_branch(&self, worktree_path: &Path) -> Result<String, String>;
    fn current_head(&self, worktree_path: &Path) -> Result<GitSha, String>;
    fn commit_local(&self, request: &CampaignCommitRequest) -> Result<GitSha, String>;
}

#[derive(Clone, Debug)]
pub struct DelegatingCampaignGit<T> {
    inner: T,
}

impl<T: GitWorktree> DelegatingCampaignGit<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: GitWorktree> GitWorktree for DelegatingCampaignGit<T> {
    fn resolve_sha(&self, request: &ResolveGitShaRequest) -> Result<GitSha, String> {
        self.inner.resolve_sha(request)
    }

    fn create(&self, request: &CreateChangeWorktreeRequest) -> Result<ChangeWorkspace, String> {
        self.inner.create(request)
    }

    fn inspect(&self, request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String> {
        self.inner.inspect(request)
    }
}

#[cfg(test)]
mod tests {
    use super::CampaignCommitRequest;
    use super::assert_campaign_git_args;
    use std::path::PathBuf;

    #[test]
    fn allows_inspect_commit_and_worktree_operations() {
        assert!(assert_campaign_git_args(&["status", "--porcelain"]).is_ok());
        assert!(assert_campaign_git_args(&["diff", "--stat"]).is_ok());
        assert!(assert_campaign_git_args(&["rev-parse", "HEAD"]).is_ok());
        assert!(
            assert_campaign_git_args(&["worktree", "add", "-b", "rack/campaign-1", "path", "abc"])
                .is_ok()
        );
        assert!(
            assert_campaign_git_args(&[
                "-c",
                "user.name=Rack AI Campaign",
                "-c",
                "user.email=rack-ai-campaign@local",
                "commit",
                "-m",
                "rack(campaign-1): step-1",
            ])
            .is_ok()
        );
        assert!(assert_campaign_git_args(&["add", "-A", "--", "src/lib.rs"]).is_ok());
        assert!(assert_campaign_git_args(&["log", "-1", "--format=%H"]).is_ok());
        assert!(assert_campaign_git_args(&["branch", "--show-current"]).is_ok());
    }

    #[test]
    fn rejects_remote_and_destructive_operations() {
        for command in [
            "push", "fetch", "pull", "remote", "merge", "rebase", "reset", "clean", "checkout",
        ] {
            let error = assert_campaign_git_args(&[command]).unwrap_err();
            assert!(error.contains(command), "{error}");
        }
    }

    #[test]
    fn commit_message_is_deterministic() {
        let request = CampaignCommitRequest::new(
            PathBuf::from("/tmp/repo"),
            "campaign-1",
            "step-1",
            vec!["src/lib.rs".to_string()],
        );
        assert_eq!(request.message(), "rack(campaign-1): step-1");
        assert_eq!(request.author_name(), "Rack AI Campaign");
        assert_eq!(request.author_email(), "rack-ai-campaign@local");
    }
}
