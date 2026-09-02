use rack_ai_domain::AcceptanceVerdict;
use rack_ai_domain::ChangeStatus;

use crate::CampaignCommitRequest;
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
use crate::ImplementWorkerRuntime;
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
    pub selected_worker: Option<ImplementWorkerRuntime>,
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
            request.document.clone(),
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
        let packet = self.execute_prepared(
            &request,
            &change_request,
            &workspace,
            ReviewPacket::from_request(&change_request).with_workspace(&workspace),
        );
        self.persist(packet)
    }

    fn execute_prepared(
        &self,
        request: &ExecuteChangeRequest,
        change_request: &ChangeRequest,
        workspace: &ChangeWorkspace,
        mut packet: ReviewPacket,
    ) -> ReviewPacket {
        packet = match self.inspect_into(change_request, workspace, packet) {
            Ok(value) => value,
            Err((packet, error)) => return fail(packet, ChangeStatus::Failed, error),
        };
        if let Some(rejected) = reject_disallowed(change_request, &packet) {
            return rejected;
        }
        if request.mode.runs_implementer() {
            packet = match self.implement(
                request.selected_worker.as_ref(),
                change_request,
                workspace,
                packet,
            ) {
                Ok(value) => value,
                Err((packet, error)) => return fail(packet, ChangeStatus::Failed, error),
            };
            if packet.status() == &ChangeStatus::ExecutorUnavailable {
                return packet;
            }
            packet = match self.inspect_into(change_request, workspace, packet) {
                Ok(value) => value,
                Err((packet, error)) => return fail(packet, ChangeStatus::Failed, error),
            };
            if let Some(rejected) = reject_disallowed(change_request, &packet) {
                return rejected;
            }
            if packet.status() == &ChangeStatus::Failed {
                return packet;
            }
        }
        if request.mode.runs_checks() {
            packet = match self.run_checks(change_request, workspace, packet.clone()) {
                Ok(value) => value,
                Err(error) => return fail(packet, ChangeStatus::Failed, error),
            };
        }
        if request.mode.runs_implementer() || request.mode.runs_checks() {
            if packet.status() != &ChangeStatus::ExecutorUnavailable {
                packet = match self.inspect_into(change_request, workspace, packet) {
                    Ok(value) => value,
                    Err((packet, error)) => return fail(packet, ChangeStatus::Failed, error),
                };
                if let Some(rejected) = reject_disallowed(change_request, &packet) {
                    return rejected;
                }
                if packet.status() == &ChangeStatus::ChecksPassed {
                    packet = match self.materialize_accepted_revision(
                        change_request,
                        workspace,
                        packet.clone(),
                    ) {
                        Ok(value) => value,
                        Err(error) => return fail(packet, ChangeStatus::Failed, error),
                    };
                    packet = packet.with_acceptance_verdict(AcceptanceVerdict::Approved);
                }
            }
        }
        packet
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
        selected_worker: Option<&ImplementWorkerRuntime>,
        request: &ChangeRequest,
        workspace: &ChangeWorkspace,
        packet: ReviewPacket,
    ) -> Result<ReviewPacket, (ReviewPacket, String)> {
        let packet = match selected_worker.and_then(ImplementWorkerRuntime::worker_provenance) {
            Some(provenance) => packet.with_worker_provenance(provenance.clone()),
            None => packet,
        };
        let Some(implementer) = self.implementer else {
            return Ok(fail(
                packet,
                ChangeStatus::ExecutorUnavailable,
                "qualified implementation harness is required for external-repository implementation"
                    .to_string(),
            ));
        };
        let implement_request = ImplementChangeRequest::new(
            workspace.worktree_path().to_path_buf(),
            request.task().value().to_string(),
        )
        .with_policy(
            request.allowed_paths().clone(),
            request.limits().timeout_seconds().value(),
        )
        .with_network_disabled(matches!(
            request.limits().network(),
            rack_ai_domain::NetworkPolicy::Disabled
        ))
        .with_max_turns(ChangeLayout::coder_max_turns());
        let implement_request = if let Some(worker) = selected_worker {
            implement_request.with_worker(worker.clone())
        } else {
            implement_request
        };
        match implementer.implement(&implement_request) {
            Ok(result) => {
                let packet = packet.with_implementer_output(result.output().to_string());
                if let Some(error) = result.protocol_error().or(result.worker_error()) {
                    return Ok(fail(packet, ChangeStatus::Failed, error.to_string()));
                }
                Ok(packet)
            }
            Err(error) => Err((packet, error)),
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
                "workspace executor is not available for external-repository command execution"
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
                .with_timeout_seconds(timeout)
                .with_environment_resources(request.environment_resources().to_vec()),
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
            .with_status(ChangeStatus::ChecksPassed))
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

    fn materialize_accepted_revision(
        &self,
        request: &ChangeRequest,
        workspace: &ChangeWorkspace,
        packet: ReviewPacket,
    ) -> Result<ReviewPacket, String> {
        let changed = source_paths(packet.changed_paths());
        if changed.is_empty() {
            return Ok(packet);
        }
        let commit_sha = self.git.commit_local(&CampaignCommitRequest::new(
            workspace.worktree_path().to_path_buf(),
            request.change_id().value(),
            "accepted-change",
            changed,
        ))?;
        Ok(packet.with_head_sha(commit_sha.value().to_string()))
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
    let source_paths = source_paths(packet.changed_paths());
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
        .with_acceptance_verdict(AcceptanceVerdict::Rejected)
        .with_last_error(Some(error))
}

fn source_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| !ChangeLayout::is_ephemeral_path(path))
        .cloned()
        .collect()
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

    use rack_ai_domain::AcceptanceVerdict;
    use rack_ai_domain::ChangeStatus;
    use rack_ai_domain::GitSha;
    use rack_ai_domain::RepositoryId;

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
                selected_worker: None,
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
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
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
                selected_worker: None,
            })
            .unwrap_err();
        assert!(error.contains("allowed paths cannot be empty"));
    }

    #[test]
    fn rejects_shell_acceptance_command() {
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
                selected_worker: None,
            })
            .unwrap_err();
        assert!(error.contains("shell interpreter"));
    }

    #[test]
    fn accepts_absolute_executable_paths_in_acceptance_command() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let mut document = sample_document(Some("a".repeat(40)));
        document.acceptance.commands = vec![vec![
            "/srv/ATHBA/.venv/bin/python".to_string(),
            "scripts/assert_test_fails.py".to_string(),
            "tests/test_reservation_book.py::test_add_duplicate_resource_id".to_string(),
            "expected failure".to_string(),
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
        let result = service
            .execute(ExecuteChangeRequest {
                document,
                mode: ChangeExecutionMode::PrepareOnly,
                selected_worker: None,
            })
            .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::Prepared);
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
        let executor = FakeExecutor::succeeding();
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ChecksOnly,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Approved)
        );
        assert_eq!(result.packet.commands().len(), 1);
    }

    #[test]
    fn forwards_environment_resources_to_acceptance_executor() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let mut document = sample_document(Some("a".repeat(40)));
        document.environment_resources = vec!["/srv/ATHBA/.venv".to_string()];
        let registry = EnvironmentRegistry;
        let result = execute_with_registry(
            &registry,
            document,
            &git,
            &manifests,
            ChangeExecutionMode::ChecksOnly,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(
            executor.seen_environment_resources(),
            vec![vec!["/srv/ATHBA/.venv".to_string()]]
        );
    }

    #[test]
    fn records_failed_acceptance_command() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::failing();
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ChecksOnly,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksFailed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
        assert!(!result.succeeded());
    }

    #[test]
    fn implements_then_approves_allowed_change() {
        let git =
            FakeGit::matching("a".repeat(40)).with_after_paths(vec!["src/lib.rs".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let implementer = FakeImplementer::successful("COMPLETE");
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Approved)
        );
        assert_eq!(result.packet.changed_paths(), ["src/lib.rs"]);
        assert_eq!(
            result.packet.implementer_output(),
            Some(&"COMPLETE".to_string())
        );
        assert_eq!(result.packet.head_sha(), "b".repeat(40));
        assert_eq!(git.commit_count(), 1);
        assert_eq!(git.committed_paths(), vec![vec!["src/lib.rs".to_string()]]);
        assert_eq!(manifests.last_saved().unwrap().head_sha(), "b".repeat(40));
    }

    #[test]
    fn no_change_accepted_execution_does_not_create_unnecessary_commit() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let implementer = FakeImplementer::successful("COMPLETE");
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(result.packet.head_sha(), "a".repeat(40));
        assert_eq!(git.commit_count(), 0);
    }

    #[test]
    fn failed_acceptance_does_not_materialize_commit() {
        let git =
            FakeGit::matching("a".repeat(40)).with_after_paths(vec!["src/lib.rs".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::failing();
        let implementer = FakeImplementer::successful("COMPLETE");
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksFailed);
        assert_eq!(git.commit_count(), 0);
    }

    #[test]
    fn worker_timeout_becomes_terminal_failed_packet_without_checks() {
        let git =
            FakeGit::matching("a".repeat(40)).with_after_paths(vec!["src/lib.rs".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let implementer = FakeImplementer::with_worker_error(
            "jcode wall-clock timeout exceeded for worker local-coder after 2 seconds",
        );
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::Failed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
        assert_eq!(result.packet.changed_paths(), ["src/lib.rs"]);
        assert!(result.packet.commands().is_empty());
        assert_eq!(
            result.packet.implementer_output(),
            Some(&"partial output".to_string())
        );
        assert!(
            result
                .packet
                .last_error()
                .unwrap()
                .contains("wall-clock timeout exceeded")
        );
        assert_eq!(git.commit_count(), 0);
    }

    #[test]
    fn post_prepare_implementer_error_persists_terminal_packet() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let implementer = FakeImplementer::with_hard_error("worker config mismatch");
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::Failed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
        assert_eq!(
            result.packet.last_error(),
            Some(&"worker config mismatch".to_string())
        );
        assert_eq!(manifests.saved_count(), 1);
    }

    #[test]
    fn accepted_revision_materialization_failure_persists_failed_packet() {
        let git = FakeGit::matching("a".repeat(40))
            .with_after_paths(vec!["src/lib.rs".to_string()])
            .with_commit_error("commit failed".to_string());
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let implementer = FakeImplementer::successful("COMPLETE");
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::Failed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
        assert_eq!(
            result.packet.last_error(),
            Some(&"commit failed".to_string())
        );
        assert_eq!(git.commit_count(), 0);
        assert_eq!(manifests.saved_count(), 1);
    }

    #[test]
    fn rejects_out_of_policy_paths_after_implement() {
        let git = FakeGit::matching("a".repeat(40))
            .with_after_paths(vec!["README.md".to_string(), "src/lib.rs".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let implementer = FakeImplementer::successful("COMPLETE");
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::PathPolicyFailed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
        assert!(result.packet.last_error().unwrap().contains("README.md"));
        assert!(result.packet.commands().is_empty());
        assert_eq!(git.commit_count(), 0);
    }

    #[test]
    fn rejects_out_of_policy_paths_after_checks() {
        let git = FakeGit::matching("a".repeat(40))
            .with_after_paths(vec!["src/lib.rs".to_string()])
            .with_after_checks_paths(vec!["src/lib.rs".to_string(), "README.md".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let implementer = FakeImplementer::successful("COMPLETE");
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::PathPolicyFailed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
        assert!(result.packet.last_error().unwrap().contains("README.md"));
        assert_eq!(result.packet.commands().len(), 1);
        assert_eq!(git.commit_count(), 0);
    }

    #[test]
    fn executes_dynamic_repository_request_through_normal_change_flow() {
        let git =
            FakeGit::matching("a".repeat(40)).with_after_paths(vec!["src/lib.rs".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let implementer = FakeImplementer::successful("COMPLETE");
        let registry = DynamicRegistry::default();
        let result = execute_with_registry(
            &registry,
            dynamic_document(Some("a".repeat(40))),
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            Some(&implementer),
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Approved)
        );
        assert_eq!(
            registry.requested_roots.borrow().as_slice(),
            &["/srv/dynamic/project".to_string()]
        );
    }

    #[test]
    fn fails_closed_when_implementer_missing() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::succeeding();
        let result = execute(
            &git,
            &manifests,
            ChangeExecutionMode::ImplementAndVerify,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ExecutorUnavailable);
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
    }

    #[test]
    fn selected_worker_provenance_is_retained_for_approved_rejected_and_timeout_packets() {
        let approved = execute_with_worker(
            &FakeGit::matching("a".repeat(40)).with_after_paths(vec!["src/lib.rs".to_string()]),
            &FakeManifests::default(),
            &FakeExecutor::succeeding(),
            &FakeImplementer::successful("COMPLETE"),
        )
        .unwrap();
        assert_eq!(approved.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(
            approved.packet.worker_provenance().unwrap().worker_id,
            "local-coder"
        );

        let rejected = execute_with_worker(
            &FakeGit::matching("a".repeat(40)).with_after_paths(vec!["src/lib.rs".to_string()]),
            &FakeManifests::default(),
            &FakeExecutor::failing(),
            &FakeImplementer::successful("COMPLETE"),
        )
        .unwrap();
        assert_eq!(rejected.packet.status(), &ChangeStatus::ChecksFailed);
        assert_eq!(
            rejected.packet.worker_provenance().unwrap().worker_role,
            "implementer-tester"
        );

        let timeout = execute_with_worker(
            &FakeGit::matching("a".repeat(40)).with_after_paths(vec!["src/lib.rs".to_string()]),
            &FakeManifests::default(),
            &FakeExecutor::succeeding(),
            &FakeImplementer::with_worker_error("worker timeout"),
        )
        .unwrap();
        assert_eq!(timeout.packet.status(), &ChangeStatus::Failed);
        assert_eq!(timeout.packet.worker_provenance().unwrap().backend, "jcode");
    }

    #[test]
    fn selected_worker_missing_harness_retains_provenance() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let policy = ApprovedCommandPolicy::default();
        let result = ExecuteChange::new(ExecuteChangeDependencies {
            registry: &SampleRegistry,
            command_policy: &policy,
            git: &git,
            manifests: &manifests,
            executor: Some(&FakeExecutor::succeeding()),
            implementer: None,
        })
        .execute(ExecuteChangeRequest {
            document: sample_document(Some("a".repeat(40))),
            mode: ChangeExecutionMode::ImplementAndVerify,
            selected_worker: Some(selected_worker()),
        })
        .unwrap();

        assert_eq!(result.packet.status(), &ChangeStatus::ExecutorUnavailable);
        assert_eq!(
            result.packet.worker_provenance().unwrap().worker_id,
            "local-coder"
        );
    }

    #[test]
    fn failure_before_worker_selection_has_unavailable_provenance() {
        let result = execute(
            &FakeGit::matching("a".repeat(40)),
            &FakeManifests::default(),
            ChangeExecutionMode::ImplementAndVerify,
            Some(&FakeExecutor::succeeding()),
            None,
        )
        .unwrap();

        assert_eq!(result.packet.worker_provenance(), None);
    }

    fn execute(
        git: &FakeGit,
        manifests: &FakeManifests,
        mode: ChangeExecutionMode,
        executor: Option<&FakeExecutor>,
        implementer: Option<&FakeImplementer>,
    ) -> Result<super::ExecuteChangeResult, String> {
        execute_with_registry(
            &SampleRegistry,
            sample_document(Some("a".repeat(40))),
            git,
            manifests,
            mode,
            executor,
            implementer,
        )
    }

    fn execute_with_registry(
        registry: &dyn RepositoryRegistry,
        document: ChangeRequestDocument,
        git: &FakeGit,
        manifests: &FakeManifests,
        mode: ChangeExecutionMode,
        executor: Option<&FakeExecutor>,
        implementer: Option<&FakeImplementer>,
    ) -> Result<super::ExecuteChangeResult, String> {
        let policy = ApprovedCommandPolicy::default();
        let service = ExecuteChange::new(ExecuteChangeDependencies {
            registry,
            command_policy: &policy,
            git,
            manifests,
            executor: executor.map(|item| item as &dyn WorkspaceExecutor),
            implementer: implementer.map(|item| item as &dyn ChangeImplementer),
        });
        service.execute(ExecuteChangeRequest {
            document,
            mode,
            selected_worker: None,
        })
    }

    fn execute_with_worker(
        git: &FakeGit,
        manifests: &FakeManifests,
        executor: &FakeExecutor,
        implementer: &FakeImplementer,
    ) -> Result<super::ExecuteChangeResult, String> {
        let policy = ApprovedCommandPolicy::default();
        ExecuteChange::new(ExecuteChangeDependencies {
            registry: &SampleRegistry,
            command_policy: &policy,
            git,
            manifests,
            executor: Some(executor),
            implementer: Some(implementer),
        })
        .execute(ExecuteChangeRequest {
            document: sample_document(Some("a".repeat(40))),
            mode: ChangeExecutionMode::ImplementAndVerify,
            selected_worker: Some(selected_worker()),
        })
    }

    fn selected_worker() -> crate::ImplementWorkerRuntime {
        crate::ImplementWorkerRuntime::new(
            "local-coder".to_string(),
            "/home/tomp/.local/bin/jcode".to_string(),
            "local-coder".to_string(),
            "local-coder".to_string(),
            "http://127.0.0.1:8018/v1".to_string(),
        )
        .with_worker_provenance(crate::WorkerExecutionProvenance {
            worker_id: "local-coder".to_string(),
            worker_role: "implementer-tester".to_string(),
            worker_kind: "jcode".to_string(),
            model_id: "eqaq-v2-local-coder".to_string(),
            provider_profile: "local-coder".to_string(),
            resource_id: "gpu-2060".to_string(),
            backend: "jcode".to_string(),
            tool_profile: Some("minimal".to_string()),
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

    fn dynamic_document(base_sha: Option<String>) -> ChangeRequestDocument {
        serde_json::from_value(serde_json::json!({
            "change_id": "job-1",
            "repository": {
                "id": "dynamic-project",
                "root": "/srv/dynamic/project",
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

    #[derive(Default)]
    struct DynamicRegistry {
        requested_roots: RefCell<Vec<String>>,
    }

    impl RepositoryRegistry for DynamicRegistry {
        fn workspace_root(&self) -> Result<WorkspaceRoot, String> {
            WorkspaceRoot::new(PathBuf::from("/srv/rack-workspaces"))
        }

        fn executor_config(&self) -> Result<ExecutorConfig, String> {
            ExecutorConfig::podman("rust:bookworm".to_string())
        }

        fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
            Err(format!("repository {} is not registered", id.value()))
        }

        fn resolve_target(
            &self,
            id: &RepositoryId,
            requested_root: Option<&std::path::Path>,
        ) -> Result<RegisteredRepository, String> {
            if id.value() != "dynamic-project" {
                return Err(format!("repository {} is not registered", id.value()));
            }
            let root = requested_root.ok_or("missing requested root".to_string())?;
            self.requested_roots
                .borrow_mut()
                .push(root.display().to_string());
            RegisteredRepository::new(id.clone(), root.to_path_buf())
        }
    }

    struct EnvironmentRegistry;

    impl RepositoryRegistry for EnvironmentRegistry {
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

        fn authorize_environment_resources(
            &self,
            requested_paths: &[String],
        ) -> Result<Vec<crate::EnvironmentResourceMount>, String> {
            requested_paths
                .iter()
                .map(|path| crate::EnvironmentResourceMount::same_path(PathBuf::from(path)))
                .collect()
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
        commit_sha: GitSha,
        inspect_count: Cell<usize>,
        commit_calls: RefCell<Vec<Vec<String>>>,
        commit_error: RefCell<Option<String>>,
        baseline_paths: Vec<String>,
        after_paths: Vec<String>,
        after_checks_paths: Option<Vec<String>>,
    }

    impl FakeGit {
        fn matching(sha: String) -> Self {
            Self {
                sha: GitSha::new(sha).unwrap(),
                commit_sha: GitSha::new("b".repeat(40)).unwrap(),
                inspect_count: Cell::new(0),
                commit_calls: RefCell::new(Vec::new()),
                commit_error: RefCell::new(None),
                baseline_paths: Vec::new(),
                after_paths: Vec::new(),
                after_checks_paths: None,
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

        fn with_after_checks_paths(mut self, after_checks_paths: Vec<String>) -> Self {
            self.after_checks_paths = Some(after_checks_paths);
            self
        }

        fn with_commit_error(self, error: String) -> Self {
            self.commit_error.replace(Some(error));
            self
        }

        fn commit_count(&self) -> usize {
            self.commit_calls.borrow().len()
        }

        fn committed_paths(&self) -> Vec<Vec<String>> {
            self.commit_calls.borrow().clone()
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
            } else if count == 2 {
                self.after_paths.clone()
            } else {
                self.after_checks_paths
                    .clone()
                    .unwrap_or_else(|| self.after_paths.clone())
            };
            Ok(GitEvidence::new(self.sha.clone(), String::new()).with_changed_paths(paths))
        }

        fn commit_local(&self, request: &crate::CampaignCommitRequest) -> Result<GitSha, String> {
            if let Some(error) = self.commit_error.borrow().clone() {
                return Err(error);
            }
            self.commit_calls
                .borrow_mut()
                .push(request.paths().to_vec());
            Ok(self.commit_sha.clone())
        }
    }

    #[derive(Default)]
    struct FakeManifests {
        saved: RefCell<Vec<String>>,
        last: RefCell<Option<ReviewPacket>>,
    }

    impl FakeManifests {
        fn last_saved(&self) -> Option<ReviewPacket> {
            self.last.borrow().clone()
        }

        fn saved_count(&self) -> usize {
            self.saved.borrow().len()
        }
    }

    impl ChangeManifestRepository for FakeManifests {
        fn save(&self, packet: &ReviewPacket) -> Result<String, String> {
            self.saved.borrow_mut().push(packet.change_id().to_string());
            *self.last.borrow_mut() = Some(packet.clone());
            Ok(format!("/tmp/{}.json", packet.change_id()))
        }
    }

    struct FakeExecutor {
        fail: bool,
        seen_environment_resources: RefCell<Vec<Vec<String>>>,
    }

    impl FakeExecutor {
        fn succeeding() -> Self {
            Self {
                fail: false,
                seen_environment_resources: RefCell::new(Vec::new()),
            }
        }

        fn failing() -> Self {
            Self {
                fail: true,
                seen_environment_resources: RefCell::new(Vec::new()),
            }
        }

        fn seen_environment_resources(&self) -> Vec<Vec<String>> {
            self.seen_environment_resources.borrow().clone()
        }
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
            self.seen_environment_resources.borrow_mut().push(
                request
                    .environment_resources()
                    .iter()
                    .map(|item| item.source_path().display().to_string())
                    .collect(),
            );
            let code = if self.fail { 1 } else { 0 };
            Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
                request.argv().to_vec(),
                code,
            )))
        }
    }

    struct FakeImplementer {
        output: String,
        worker_error: Option<String>,
        hard_error: Option<String>,
    }

    impl FakeImplementer {
        fn successful(output: &str) -> Self {
            Self {
                output: output.to_string(),
                worker_error: None,
                hard_error: None,
            }
        }

        fn with_worker_error(error: &str) -> Self {
            Self {
                output: "partial output".to_string(),
                worker_error: Some(error.to_string()),
                hard_error: None,
            }
        }

        fn with_hard_error(error: &str) -> Self {
            Self {
                output: String::new(),
                worker_error: None,
                hard_error: Some(error.to_string()),
            }
        }
    }

    impl ChangeImplementer for FakeImplementer {
        fn implement(
            &self,
            _request: &ImplementChangeRequest,
        ) -> Result<ImplementChangeResult, String> {
            if let Some(error) = &self.hard_error {
                return Err(error.clone());
            }
            let mut result = ImplementChangeResult::new(self.output.clone());
            if let Some(error) = &self.worker_error {
                result = result.with_worker_error(error.clone());
            }
            Ok(result)
        }
    }
}
