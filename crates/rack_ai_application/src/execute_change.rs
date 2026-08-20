use rack_ai_domain::ChangeStatus;

use crate::ChangeManifestRepository;
use crate::ChangeRequest;
use crate::ChangeRequestDocument;
use crate::ChangeRequestResolution;
use crate::CommandPolicy;
use crate::GitWorktree;
use crate::InspectChangeWorktreeRequest;
use crate::PrepareChange;
use crate::PrepareChangeDependencies;
use crate::ReadFileRequest;
use crate::RepositoryRegistry;
use crate::ReviewPacket;
use crate::RunCommandRequest;
use crate::WorkspaceExecutor;
use crate::WorkspacePath;

pub struct ExecuteChange<'a> {
    registry: &'a dyn RepositoryRegistry,
    command_policy: &'a dyn CommandPolicy,
    git: &'a dyn GitWorktree,
    manifests: &'a dyn ChangeManifestRepository,
    executor: Option<&'a dyn WorkspaceExecutor>,
}

pub struct ExecuteChangeDependencies<'a> {
    pub registry: &'a dyn RepositoryRegistry,
    pub command_policy: &'a dyn CommandPolicy,
    pub git: &'a dyn GitWorktree,
    pub manifests: &'a dyn ChangeManifestRepository,
    pub executor: Option<&'a dyn WorkspaceExecutor>,
}

pub struct ExecuteChangeRequest {
    pub document: ChangeRequestDocument,
    pub run_checks: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecuteChangeResult {
    pub packet: ReviewPacket,
    pub packet_path: String,
}

impl<'a> ExecuteChange<'a> {
    pub fn new(dependencies: ExecuteChangeDependencies<'a>) -> Self {
        Self {
            registry: dependencies.registry,
            command_policy: dependencies.command_policy,
            git: dependencies.git,
            manifests: dependencies.manifests,
            executor: dependencies.executor,
        }
    }

    pub fn execute(&self, request: ExecuteChangeRequest) -> Result<ExecuteChangeResult, String> {
        let change_request = ChangeRequest::from_document(
            request.document,
            &ChangeRequestResolution {
                registry: self.registry,
                command_policy: self.command_policy,
                git: self.git,
            },
        )?;
        let workspace = PrepareChange::new(PrepareChangeDependencies {
            registry: self.registry,
            git: self.git,
        })
        .execute(&change_request)?;
        let evidence = self.git.inspect(&InspectChangeWorktreeRequest::new(
            workspace.worktree_path().to_path_buf(),
            change_request.repository().base_sha().clone(),
        ))?;
        let mut packet = ReviewPacket::from_request(&change_request)
            .with_workspace(&workspace)
            .with_git_evidence(&evidence);
        let disallowed = change_request
            .allowed_paths()
            .reject_disallowed(evidence.changed_paths());
        if !disallowed.is_empty() {
            packet = packet
                .with_status(ChangeStatus::PathPolicyFailed)
                .with_last_error(Some(format!(
                    "changed paths outside allowed_paths: {}",
                    disallowed
                        .into_iter()
                        .map(|path| path.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            return self.persist(packet);
        }
        if request.run_checks {
            packet = self.run_checks(&change_request, &workspace, packet)?;
        }
        self.persist(packet)
    }

    fn run_checks(
        &self,
        request: &ChangeRequest,
        workspace: &crate::ChangeWorkspace,
        packet: ReviewPacket,
    ) -> Result<ReviewPacket, String> {
        let Some(executor) = self.executor else {
            return Ok(packet
                .with_status(ChangeStatus::ExecutorUnavailable)
                .with_last_error(Some(
                    "podman is not available; rootless Podman is required for external-repository command execution"
                        .to_string(),
                )));
        };
        let timeout = request.limits().timeout_seconds().value();
        let mut commands = Vec::new();
        for command in request.acceptance().commands() {
            let result = executor.run_command(
                &RunCommandRequest::new(
                    workspace.worktree_path().to_path_buf(),
                    command.argv().to_vec(),
                )?
                .with_timeout_seconds(timeout),
            );
            match result {
                Ok(execution) => commands.push(execution.evidence().clone()),
                Err(error) => {
                    return Ok(packet
                        .with_commands(commands)
                        .with_status(check_status(&error))
                        .with_last_error(Some(error)));
                }
            }
        }
        if let Some(failed) = commands.iter().find(|item| !item.succeeded()) {
            return Ok(packet
                .with_commands(commands.clone())
                .with_status(ChangeStatus::ChecksFailed)
                .with_last_error(Some(format!(
                    "acceptance command failed: {}",
                    failed.argv().join(" ")
                ))));
        }
        if let Err(error) = self.assert_artifacts(executor, request, workspace) {
            return Ok(packet
                .with_commands(commands)
                .with_status(ChangeStatus::ChecksFailed)
                .with_last_error(Some(error)));
        }
        Ok(packet
            .with_commands(commands)
            .with_status(ChangeStatus::ChecksPassed))
    }

    fn assert_artifacts(
        &self,
        executor: &dyn WorkspaceExecutor,
        request: &ChangeRequest,
        workspace: &crate::ChangeWorkspace,
    ) -> Result<(), String> {
        for artifact in request.acceptance().required_artifacts() {
            executor.read_file(&ReadFileRequest::new(
                workspace.worktree_path().to_path_buf(),
                WorkspacePath::parse(artifact.value())?,
            ))?;
        }
        Ok(())
    }

    fn persist(&self, packet: ReviewPacket) -> Result<ExecuteChangeResult, String> {
        let packet_path = self.manifests.save(&packet)?;
        Ok(ExecuteChangeResult {
            packet,
            packet_path,
        })
    }
}

fn check_status(error: &str) -> ChangeStatus {
    if error.contains("podman is not available") {
        ChangeStatus::ExecutorUnavailable
    } else {
        ChangeStatus::Failed
    }
}

impl ExecuteChangeResult {
    pub fn succeeded(&self) -> bool {
        self.packet.status().is_successful()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use rack_ai_domain::ChangeStatus;
    use rack_ai_domain::GitSha;
    use rack_ai_domain::RepositoryId;

    use super::ExecuteChange;
    use super::ExecuteChangeDependencies;
    use super::ExecuteChangeRequest;
    use crate::ApprovedCommandPolicy;
    use crate::ChangeManifestRepository;
    use crate::ChangeRequestDocument;
    use crate::ChangeWorkspace;
    use crate::CommandEvidence;
    use crate::CreateChangeWorktreeRequest;
    use crate::ExecutorConfig;
    use crate::GitEvidence;
    use crate::GitWorktree;
    use crate::InspectChangeWorktreeRequest;
    use crate::ReadFileRequest;
    use crate::RegisteredRepository;
    use crate::RepositoryRegistry;
    use crate::ResolveGitShaRequest;
    use crate::ReviewPacket;
    use crate::RunCommandRequest;
    use crate::WorkspaceExecutionResult;
    use crate::WorkspaceExecutor;
    use crate::WorkspaceRoot;
    use crate::WriteFileRequest;

    #[test]
    fn prepares_workspace_and_records_evidence() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let result = execute(&git, &manifests, false, None).unwrap();
        assert!(result.succeeded());
        assert_eq!(result.packet.status(), &ChangeStatus::Prepared);
        assert_eq!(result.packet.base_sha(), "a".repeat(40));
        assert_eq!(result.packet.branch(), "rack/change-job-1");
    }

    #[test]
    fn rejects_unregistered_repository() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let registry = EmptyRegistry;
        let policy = ApprovedCommandPolicy::default();
        let service = ExecuteChange::new(ExecuteChangeDependencies {
            registry: &registry,
            command_policy: &policy,
            git: &git,
            manifests: &manifests,
            executor: None,
        });
        let error = service
            .execute(ExecuteChangeRequest {
                document: sample_document(None),
                run_checks: false,
            })
            .unwrap_err();
        assert!(error.contains("not registered"));
    }

    #[test]
    fn rejects_sha_mismatch() {
        let git = FakeGit::matching("b".repeat(40));
        let manifests = FakeManifests::default();
        let error = execute(&git, &manifests, false, None).unwrap_err();
        assert!(error.contains("base sha does not match"));
    }

    #[test]
    fn rejects_disallowed_changed_paths() {
        let git =
            FakeGit::matching("a".repeat(40)).with_changed_paths(vec!["README.md".to_string()]);
        let manifests = FakeManifests::default();
        let result = execute(&git, &manifests, false, None).unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::PathPolicyFailed);
        assert!(!result.succeeded());
    }

    #[test]
    fn rejects_empty_allowed_paths() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let mut document = sample_document(Some("a".repeat(40)));
        document.allowed_paths = vec![];
        let registry = SampleRegistry;
        let policy = ApprovedCommandPolicy::default();
        let service = ExecuteChange::new(ExecuteChangeDependencies {
            registry: &registry,
            command_policy: &policy,
            git: &git,
            manifests: &manifests,
            executor: None,
        });
        let error = service
            .execute(ExecuteChangeRequest {
                document,
                run_checks: false,
            })
            .unwrap_err();
        assert!(error.contains("allowed paths cannot be empty"));
    }

    #[test]
    fn rejects_unapproved_acceptance_command() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let mut document = sample_document(Some("a".repeat(40)));
        document.acceptance.commands = vec![vec![
            "bash".to_string(),
            "-c".to_string(),
            "rm -rf /".to_string(),
        ]];
        let registry = SampleRegistry;
        let policy = ApprovedCommandPolicy::default();
        let service = ExecuteChange::new(ExecuteChangeDependencies {
            registry: &registry,
            command_policy: &policy,
            git: &git,
            manifests: &manifests,
            executor: None,
        });
        let error = service
            .execute(ExecuteChangeRequest {
                document,
                run_checks: false,
            })
            .unwrap_err();
        assert!(error.contains("not approved") || error.contains("approved program"));
    }

    #[test]
    fn fails_closed_when_checks_require_missing_executor() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let result = execute(&git, &manifests, true, None).unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ExecutorUnavailable);
    }

    #[test]
    fn runs_acceptance_commands_through_executor() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor { fail: false };
        let result = execute(&git, &manifests, true, Some(&executor)).unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(result.packet.commands().len(), 1);
    }

    #[test]
    fn records_failed_acceptance_command() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor { fail: true };
        let result = execute(&git, &manifests, true, Some(&executor)).unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksFailed);
        assert!(!result.succeeded());
    }

    fn execute(
        git: &FakeGit,
        manifests: &FakeManifests,
        run_checks: bool,
        executor: Option<&FakeExecutor>,
    ) -> Result<super::ExecuteChangeResult, String> {
        let registry = SampleRegistry;
        let policy = ApprovedCommandPolicy::default();
        let service = ExecuteChange::new(ExecuteChangeDependencies {
            registry: &registry,
            command_policy: &policy,
            git,
            manifests,
            executor: executor.map(|item| item as &dyn WorkspaceExecutor),
        });
        service.execute(ExecuteChangeRequest {
            document: sample_document(Some("a".repeat(40))),
            run_checks,
        })
    }

    fn sample_document(base_sha: Option<String>) -> ChangeRequestDocument {
        serde_json::from_value(serde_json::json!({
            "change_id": "job-1",
            "repository": {
                "id": "adaptos",
                "registered_root": "/srv/projects/adaptos",
                "base_ref": "main",
                "base_sha": base_sha
            },
            "task": "Add a bounded feature with tests.",
            "allowed_paths": ["src/", "Cargo.toml"],
            "acceptance": {"commands": [["cargo", "test"]]},
            "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
        }))
        .unwrap()
    }

    struct SampleRegistry;

    impl RepositoryRegistry for SampleRegistry {
        fn workspace_root(&self) -> Result<WorkspaceRoot, String> {
            WorkspaceRoot::new(PathBuf::from("/srv/rack-workspaces"))
        }

        fn executor_config(&self) -> Result<ExecutorConfig, String> {
            ExecutorConfig::podman("rust:bookworm".to_string())
        }

        fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
            if id.value() != "adaptos" {
                return Err(format!("repository {} is not registered", id.value()));
            }
            RegisteredRepository::new(id.clone(), PathBuf::from("/srv/projects/adaptos"))
        }
    }

    struct EmptyRegistry;

    impl RepositoryRegistry for EmptyRegistry {
        fn workspace_root(&self) -> Result<WorkspaceRoot, String> {
            WorkspaceRoot::new(PathBuf::from("/srv/rack-workspaces"))
        }

        fn executor_config(&self) -> Result<ExecutorConfig, String> {
            ExecutorConfig::podman("rust:bookworm".to_string())
        }

        fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
            Err(format!("repository {} is not registered", id.value()))
        }
    }

    struct FakeGit {
        sha: GitSha,
        changed_paths: Vec<String>,
    }

    impl FakeGit {
        fn matching(sha: String) -> Self {
            Self {
                sha: GitSha::new(sha).unwrap(),
                changed_paths: Vec::new(),
            }
        }

        fn with_changed_paths(mut self, changed_paths: Vec<String>) -> Self {
            self.changed_paths = changed_paths;
            self
        }
    }

    impl GitWorktree for FakeGit {
        fn resolve_sha(&self, _request: &ResolveGitShaRequest) -> Result<GitSha, String> {
            Ok(self.sha.clone())
        }

        fn create(&self, request: &CreateChangeWorktreeRequest) -> Result<ChangeWorkspace, String> {
            Ok(ChangeWorkspace::new(
                rack_ai_domain::ChangeId::new("job-1".to_string()).unwrap(),
                request.worktree_path().to_path_buf(),
            )
            .with_branch_name(request.branch_name().to_string())
            .with_base_sha(request.base_sha().clone()))
        }

        fn inspect(&self, request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String> {
            if request.expected_base_sha() != &self.sha {
                return Err("worktree is not at the recorded base sha".to_string());
            }
            Ok(GitEvidence::new(self.sha.clone(), String::new())
                .with_changed_paths(self.changed_paths.clone()))
        }
    }

    #[derive(Default)]
    struct FakeManifests {
        saved: RefCell<Vec<String>>,
    }

    impl ChangeManifestRepository for FakeManifests {
        fn save(&self, packet: &ReviewPacket) -> Result<String, String> {
            self.saved.borrow_mut().push(packet.change_id().to_string());
            Ok(format!("/tmp/{}.json", packet.change_id()))
        }
    }

    struct FakeExecutor {
        fail: bool,
    }

    impl WorkspaceExecutor for FakeExecutor {
        fn write_file(
            &self,
            _request: &WriteFileRequest,
        ) -> Result<WorkspaceExecutionResult, String> {
            Err("unused".to_string())
        }

        fn read_file(
            &self,
            _request: &ReadFileRequest,
        ) -> Result<WorkspaceExecutionResult, String> {
            Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
                vec!["read".to_string()],
                0,
            )))
        }

        fn run_command(
            &self,
            request: &RunCommandRequest,
        ) -> Result<WorkspaceExecutionResult, String> {
            let code = if self.fail { 1 } else { 0 };
            Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
                request.argv().to_vec(),
                code,
            )))
        }
    }
}
