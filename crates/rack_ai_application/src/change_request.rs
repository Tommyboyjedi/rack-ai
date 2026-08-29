use std::path::Path;

use rack_ai_domain::AcceptanceCommand;
use rack_ai_domain::AllowedPath;
use rack_ai_domain::AllowedPaths;
use rack_ai_domain::AttemptLimit;
use rack_ai_domain::ChangeId;
use rack_ai_domain::ChangeLimits;
use rack_ai_domain::ChangeTask;
use rack_ai_domain::GitRef;
use rack_ai_domain::GitSha;
use rack_ai_domain::NetworkPolicy;
use rack_ai_domain::RepositoryId;
use rack_ai_domain::RequiredArtifact;
use rack_ai_domain::TimeoutSeconds;

use crate::AcceptancePolicy;
use crate::ChangeRepositoryTarget;
use crate::ChangeRequestDocument;
use crate::ChangeRequestResolution;
use crate::CommandPolicy;
use crate::ResolveGitShaRequest;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRequest {
    change_id: ChangeId,
    repository: ChangeRepositoryTarget,
    task: ChangeTask,
    allowed_paths: AllowedPaths,
    acceptance: AcceptancePolicy,
    limits: ChangeLimits,
}

impl ChangeRequest {
    pub fn from_document(
        document: ChangeRequestDocument,
        resolution: &ChangeRequestResolution,
    ) -> Result<Self, String> {
        assert_root_fields_are_consistent(&document)?;
        let change_id = ChangeId::new(document.change_id)?;
        let repository_id = RepositoryId::new(document.repository.id)?;
        let requested_root = document.repository.root.as_deref().map(Path::new);
        let registered = resolution
            .registry
            .resolve_target(&repository_id, requested_root)?;
        if !registered.enabled() {
            return Err(format!("repository {} is disabled", repository_id.value()));
        }
        if let Some(declared_root) = document.repository.registered_root {
            if declared_root != registered.root().to_string_lossy() {
                return Err(format!(
                    "registered_root does not match repository registry for {}",
                    repository_id.value()
                ));
            }
        }
        let base_ref = GitRef::new(document.repository.base_ref)?;
        let resolved_sha = resolution.git.resolve_sha(&ResolveGitShaRequest::new(
            registered.root().to_path_buf(),
            base_ref.clone(),
        ))?;
        let base_sha = if let Some(declared) = document.repository.base_sha {
            let declared_sha = GitSha::new(declared)?;
            if declared_sha != resolved_sha {
                return Err(
                    "base sha does not match the registered repository baseline".to_string()
                );
            }
            declared_sha
        } else {
            resolved_sha
        };
        let allowed_paths = AllowedPaths::new(
            document
                .allowed_paths
                .into_iter()
                .map(AllowedPath::new)
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        let commands = document
            .acceptance
            .commands
            .into_iter()
            .map(AcceptanceCommand::new)
            .collect::<Result<Vec<_>, _>>()?;
        assert_commands_allowed(resolution.command_policy, &commands)?;
        let artifacts = document
            .acceptance
            .required_artifacts
            .into_iter()
            .map(RequiredArtifact::new)
            .collect::<Result<Vec<_>, _>>()?;
        let limits = ChangeLimits::new(
            AttemptLimit::new(document.limits.max_implementation_attempts)?,
            TimeoutSeconds::new(document.limits.timeout_seconds)?,
        )
        .with_network(NetworkPolicy::parse(&document.limits.network)?);
        Ok(Self {
            change_id,
            repository: ChangeRepositoryTarget::new(
                repository_id,
                registered.root().to_path_buf(),
            )?
            .with_base_ref(base_ref)
            .with_base_sha(base_sha),
            task: ChangeTask::new(document.task)?,
            allowed_paths,
            acceptance: AcceptancePolicy::new(commands)?.with_required_artifacts(artifacts),
            limits,
        })
    }

    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    pub fn repository(&self) -> &ChangeRepositoryTarget {
        &self.repository
    }

    pub fn task(&self) -> &ChangeTask {
        &self.task
    }

    pub fn allowed_paths(&self) -> &AllowedPaths {
        &self.allowed_paths
    }

    pub fn acceptance(&self) -> &AcceptancePolicy {
        &self.acceptance
    }

    pub fn limits(&self) -> &ChangeLimits {
        &self.limits
    }
}

fn assert_root_fields_are_consistent(document: &ChangeRequestDocument) -> Result<(), String> {
    if document.repository.registered_root.is_some() && document.repository.root.is_some() {
        return Err("repository must not specify both registered_root and root".to_string());
    }
    Ok(())
}

fn assert_commands_allowed(
    policy: &dyn CommandPolicy,
    commands: &[AcceptanceCommand],
) -> Result<(), String> {
    for command in commands {
        policy.assert_allowed(command)?;
    }
    Ok(())
}
