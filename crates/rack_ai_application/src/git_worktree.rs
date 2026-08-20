use rack_ai_domain::GitSha;

use crate::ChangeWorkspace;
use crate::CreateChangeWorktreeRequest;
use crate::GitEvidence;
use crate::InspectChangeWorktreeRequest;
use crate::ResolveGitShaRequest;

pub trait GitWorktree {
    fn resolve_sha(&self, request: &ResolveGitShaRequest) -> Result<GitSha, String>;
    fn create(&self, request: &CreateChangeWorktreeRequest) -> Result<ChangeWorkspace, String>;
    fn inspect(&self, request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String>;
}
