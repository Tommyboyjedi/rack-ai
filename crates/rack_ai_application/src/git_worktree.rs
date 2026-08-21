use std::path::Path;

use rack_ai_domain::GitSha;

use crate::CampaignCommitRequest;
use crate::ChangeWorkspace;
use crate::CreateChangeWorktreeRequest;
use crate::GitEvidence;
use crate::InspectChangeWorktreeRequest;
use crate::ResolveGitShaRequest;

pub trait GitWorktree {
    fn resolve_sha(&self, request: &ResolveGitShaRequest) -> Result<GitSha, String>;
    fn create(&self, request: &CreateChangeWorktreeRequest) -> Result<ChangeWorkspace, String>;
    fn inspect(&self, request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String>;

    fn snapshot(&self, _worktree_path: &Path) -> Result<GitEvidence, String> {
        Err("git snapshot is not supported".to_string())
    }

    fn current_branch(&self, _worktree_path: &Path) -> Result<String, String> {
        Err("git current_branch is not supported".to_string())
    }

    fn current_head(&self, _worktree_path: &Path) -> Result<GitSha, String> {
        Err("git current_head is not supported".to_string())
    }

    fn commit_local(&self, _request: &CampaignCommitRequest) -> Result<GitSha, String> {
        Err("git commit_local is not supported".to_string())
    }
}
