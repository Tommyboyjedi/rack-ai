use rack_ai_domain::ChangeStatus;
use rack_ai_domain::VerifierVerdict;

use crate::ChangeExecutionMode;
use crate::ChangeImplementer;
use crate::ChangeLayout;
use crate::ChangeManifestRepository;
use crate::ChangeRequest;
use crate::ChangeRequestDocument;
use crate::ChangeRequestResolution;
use crate::ChangeWorkspace;
use crate::CommandPolicy;
use crate::GitWorktree;
use crate::ImplementChangeRequest;
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
    implementer: Option<&'a dyn ChangeImplementer>,
}

pub struct ExecuteChangeDependencies<'a> {
    pub registry: &'a dyn RepositoryRegistry,
    pub command_policy: &'a dyn CommandPolicy,
    pub git: &'a dyn GitWorktree,
    pub manifests: &'a dyn ChangeManifestRepository,
    pub executor: Option<&'a dyn WorkspaceExecutor>,
    pub implementer: Option<&'a dyn ChangeImplementer>,
}

pub struct ExecuteChangeRequest {
    pub document: ChangeRequestDocument,
    pub mode: ChangeExecutionMode,
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
            implementer: dependencies.implementer,
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
        let mut packet = ReviewPacket::from_request(&change_request).with_workspace(&workspace);
        packet = match self.inspect_into(&change_request, &workspace, packet) {
            Ok(value) => value,
            Err((packet, error)) => {
                return self.persist(fail(packet, ChangeStatus::Failed, error));
            }
        };
        if let Some(rejected) = reject_disallowed(&change_request, &packet) {
            return self.persist(rejected);
        }
        if request.mode.runs_implementer() {
            packet = self.implement(&change_request, &workspace, packet)?;
            if packet.status() == &ChangeStatus::ExecutorUnavailable {
                return self.persist(packet);
            }
            packet = match self.inspect_into(&change_request, &workspace, packet) {
                Ok(value) => value,
                Err((packet, error)) => {
                    return self.persist(fail(packet, ChangeStatus::Failed, error));
                }
            };
            if let Some(rejected) = reject_disallowed(&change_request, &packet) {
                return self.persist(rejected);
            }
            if packet.status() == &ChangeStatus::Failed {
                return self.persist(packet);
            }
        }
        if request.mode.runs_checks() {
            packet = self.run_checks(&change_request, &workspace, packet)?;
        }
        self.persist(packet)
    }

    fn inspect_into(
        &self,
        request: &ChangeRequest,
        workspace: &ChangeWorkspace,
        packet: ReviewPacket,
    ) -> Result<ReviewPacket, (ReviewPacket, String)> {
        match self.git.inspect(&InspectChangeWorktreeRequest::new(
            workspace.worktree_path().to_path_buf(),
            request.repository().base_sha().clone(),
        )) {
            Ok(evidence) => Ok(packet.with_git_evidence(&evidence)),
            Err(error) => Err((packet, error)),
        }
    }

    fn implement(
        &self,
        request: &ChangeRequest,
        workspace: &ChangeWorkspace,
        packet: ReviewPacket,
    ) -> Result<ReviewPacket, String> {
        let Some(implementer) = self.implementer else {
            return Ok(fail(
                packet,
                ChangeStatus::ExecutorUnavailable,
                "podman-backed coder is required for external-repository implementation"
                    .to_string(),
            ));
        };
        match implementer.implement(
            &ImplementChangeRequest::new(
                workspace.worktree_path().to_path_buf(),
                request.task().value().to_string(),
            )
            .with_policy(
                request.allowed_paths().clone(),
                request.limits().timeout_seconds().value(),
            )
            .with_max_turns(ChangeLayout::coder_max_turns()),
        ) {
            Ok(result) => Ok(packet.with_implementer_output(result.output().to_string())),
            Err(error) => Ok(fail(packet, ChangeStatus::Failed, error)),
        }
    }

    fn run_checks(
        &self,
        request: &ChangeRequest,
        workspace: &ChangeWorkspace,
        packet: ReviewPacket,
    ) -> Result<ReviewPacket, String> {
        if packet.status() == &ChangeStatus::Failed
            || packet.status() == &ChangeStatus::PathPolicyFailed
            || packet.status() == &ChangeStatus::ExecutorUnavailable
        {
            return Ok(packet);
        }
        let Some(executor) = self.executor else {
            return Ok(fail(
                packet,
                ChangeStatus::ExecutorUnavailable,
                "podman is not available; rootless Podman is required for external-repository command execution"
                    .to_string(),
            ));
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
                    return Ok(fail(
                        packet.with_commands(commands),
                        check_status(&error),
                        error,
                    ));
                }
            }
        }
        if let Some(failed) = commands.iter().find(|item| !item.succeeded()) {
            let message = if failed.timed_out() {
                format!(
                    "acceptance command exceeded wall-clock timeout: {}",
                    failed.argv().join(" ")
                )
            } else {
                format!("acceptance command failed: {}", failed.argv().join(" "))
            };
            return Ok(fail(
                packet.with_commands(commands.clone()),
                ChangeStatus::ChecksFailed,
                message,
            ));
        }
        if let Err(error) = self.assert_artifacts(executor, request, workspace) {
            return Ok(fail(
                packet.with_commands(commands),
                ChangeStatus::ChecksFailed,
                error,
            ));
        }
        Ok(packet
            .with_commands(commands)
            .with_status(ChangeStatus::ChecksPassed)
            .with_verdict(VerifierVerdict::Approved))
    }

    fn assert_artifacts(
        &self,
        executor: &dyn WorkspaceExecutor,
        request: &ChangeRequest,
        workspace: &ChangeWorkspace,
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

fn reject_disallowed(request: &ChangeRequest, packet: &ReviewPacket) -> Option<ReviewPacket> {
    let source_paths = packet
        .changed_paths()
        .iter()
        .filter(|path| !ChangeLayout::is_ephemeral_path(path))
        .cloned()
        .collect::<Vec<_>>();
    let disallowed = request.allowed_paths().reject_disallowed(&source_paths);
    if disallowed.is_empty() {
        return None;
    }
    Some(fail(
        packet.clone(),
        ChangeStatus::PathPolicyFailed,
        format!(
            "changed paths outside allowed_paths: {}",
            disallowed
                .into_iter()
                .map(|path| path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ))
}

fn fail(packet: ReviewPacket, status: ChangeStatus, error: String) -> ReviewPacket {
    packet
        .with_status(status)
        .with_verdict(VerifierVerdict::Rejected)
        .with_last_error(Some(error))
}

fn check_status(error: &str) -> ChangeStatus {
    if error.contains("podman is not available") || error.contains("not running rootless") {
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
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::path::PathBuf;

    use rack_ai_domain::ChangeStatus;
    use rack_ai_domain::GitSha;
    use rack_ai_domain::RepositoryId;
    use rack_ai_domain::VerifierVerdict;

    use super::ExecuteChange;
    use super::ExecuteChangeDependencies;
    use super::ExecuteChangeRequest;
    use crate::ApprovedCommandPolicy;
    use crate::ChangeExecutionMode;
    use crate::ChangeImplementer;
    use crate::ChangeManifestRepository;
    use crate::ChangeRequestDocument;
    use crate::ChangeWorkspace;
    use crate::CommandEvidence;
    use crate::CreateChangeWorktreeRequest;
    use crate::ExecutorConfig;
    use crate::GitEvidence;
    use crate::GitWorktree;
    use crate::ImplementChangeRequest;
    use crate::ImplementChangeResult;
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
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::PrepareOnly,
            None,
            None,
        )
        .unwrap();
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
            implementer: None,
        });
        let error = service
            .execute(ExecuteChangeRequest {
                document: sample_document(None),
                mode: ChangeExecutionMode::PrepareOnly,
            })
            .unwrap_err();
        assert!(error.contains("not registered"));
    }

    #[test]
    fn rejects_sha_mismatch() {
        let git = FakeGit::matching("b".repeat(40));
        let manifests = FakeManifests::default();
        let error = execute(
            &git,
            &manifests,
            ChangeExecutionMode::PrepareOnly,
            None,
            None,
        )
        .unwrap_err();
        assert!(error.contains("base sha does not match"));
    }

    #[test]
    fn rejects_disallowed_changed_paths() {
        let git =
            FakeGit::matching("a".repeat(40)).with_baseline_paths(vec!["README.md".to_string()]);
        let manifests = FakeManifests::default();
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::PrepareOnly,
            None,
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::PathPolicyFailed);
        assert_eq!(result.packet.verdict(), Some(&VerifierVerdict::Rejected));
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
            implementer: None,
        });
        let error = service
            .execute(ExecuteChangeRequest {
                document,
                mode: ChangeExecutionMode::PrepareOnly,
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
            implementer: None,
        });
        let error = service
            .execute(ExecuteChangeRequest {
                document,
                mode: ChangeExecutionMode::PrepareOnly,
            })
            .unwrap_err();
        assert!(error.contains("not approved") || error.contains("approved program"));
    }

    #[test]
    fn fails_closed_when_checks_require_missing_executor() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ChecksOnly,
            None,
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ExecutorUnavailable);
    }

    #[test]
    fn runs_acceptance_commands_through_executor() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor { fail: false };
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ChecksOnly,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(result.packet.verdict(), Some(&VerifierVerdict::Approved));
        assert_eq!(result.packet.commands().len(), 1);
    }

    #[test]
    fn records_failed_acceptance_command() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor { fail: true };
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ChecksOnly,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksFailed);
        assert_eq!(result.packet.verdict(), Some(&VerifierVerdict::Rejected));
        assert!(!result.succeeded());
    }

    #[test]
    fn implements_then_approves_allowed_change() {
        let git =
            FakeGit::matching("a".repeat(40)).with_after_paths(vec!["src/lib.rs".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor { fail: false };
        let implementer = FakeImplementer {
            output: "COMPLETE".to_string(),
        };
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(result.packet.verdict(), Some(&VerifierVerdict::Approved));
        assert_eq!(result.packet.changed_paths(), ["src/lib.rs"]);
        assert_eq!(
            result.packet.implementer_output(),
            Some(&"COMPLETE".to_string())
        );
    }

    #[test]
    fn rejects_out_of_policy_paths_after_implement() {
        let git = FakeGit::matching("a".repeat(40))
            .with_after_paths(vec!["README.md".to_string(), "src/lib.rs".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor { fail: false };
        let implementer = FakeImplementer {
            output: "COMPLETE".to_string(),
        };
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::PathPolicyFailed);
        assert_eq!(result.packet.verdict(), Some(&VerifierVerdict::Rejected));
        assert!(result.packet.last_error().unwrap().contains("README.md"));
        assert!(result.packet.commands().is_empty());
    }

    #[test]
    fn fails_closed_when_implementer_missing() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor { fail: false };
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ExecutorUnavailable);
        assert_eq!(result.packet.verdict(), Some(&VerifierVerdict::Rejected));
    }

    fn execute(
        git: &FakeGit,
        manifests: &FakeManifests,
        mode: ChangeExecutionMode,
        executor: Option<&FakeExecutor>,
        implementer: Option<&FakeImplementer>,
    ) -> Result<super::ExecuteChangeResult, String> {
        let registry = SampleRegistry;
        let policy = ApprovedCommandPolicy::default();
        let service = ExecuteChange::new(ExecuteChangeDependencies {
            registry: &registry,
            command_policy: &policy,
            git,
            manifests,
            executor: executor.map(|item| item as &dyn WorkspaceExecutor),
            implementer: implementer.map(|item| item as &dyn ChangeImplementer),
        });
        service.execute(ExecuteChangeRequest {
            document: sample_document(Some("a".repeat(40))),
            mode,
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
        inspect_count: Cell<usize>,
        baseline_paths: Vec<String>,
        after_paths: Vec<String>,
    }

    impl FakeGit {
        fn matching(sha: String) -> Self {
            Self {
                sha: GitSha::new(sha).unwrap(),
                inspect_count: Cell::new(0),
                baseline_paths: Vec::new(),
                after_paths: Vec::new(),
            }
        }

        fn with_baseline_paths(mut self, baseline_paths: Vec<String>) -> Self {
            self.baseline_paths = baseline_paths;
            self
        }

        fn with_after_paths(mut self, after_paths: Vec<String>) -> Self {
            self.after_paths = after_paths;
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
            let count = self.inspect_count.get() + 1;
            self.inspect_count.set(count);
            let paths = if count == 1 {
                self.baseline_paths.clone()
            } else {
                self.after_paths.clone()
            };
            Ok(GitEvidence::new(self.sha.clone(), String::new()).with_changed_paths(paths))
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

    struct FakeImplementer {
        output: String,
    }

    impl ChangeImplementer for FakeImplementer {
        fn implement(
            &self,
            _request: &ImplementChangeRequest,
        ) -> Result<ImplementChangeResult, String> {
            Ok(ImplementChangeResult::new(self.output.clone()))
        }
    }
}
