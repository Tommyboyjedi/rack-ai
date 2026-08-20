use crate::CommandPolicy;
use crate::GitWorktree;
use crate::RepositoryRegistry;

pub struct ChangeRequestResolution<'a> {
    pub registry: &'a dyn RepositoryRegistry,
    pub command_policy: &'a dyn CommandPolicy,
    pub git: &'a dyn GitWorktree,
}
