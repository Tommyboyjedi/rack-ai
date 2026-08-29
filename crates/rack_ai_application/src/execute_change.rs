use rack_ai_domain::AcceptanceVerdict;
use rack_ai_domain::ChangeStatus;

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

const PYTHON_EXECUTABLE_PROBE: [&str; 3] = ["python3", "-c", "import sys; print(sys.executable)"];
const PYTHON_VERSION_PROBE: [&str; 2] = ["python3", "--version"];
const PYTEST_VERSION_PROBE: [&str; 4] = ["python3", "-m", "pytest", "--version"];

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
            packet = self.implement(
                request.selected_worker.as_ref(),
                &change_request,
                &workspace,
                packet,
            )?;
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
        if request.mode.runs_implementer() || request.mode.runs_checks() {
            if packet.status() != &ChangeStatus::ExecutorUnavailable {
                packet = match self.inspect_into(&change_request, &workspace, packet) {
                    Ok(value) => value,
                    Err((packet, error)) => {
                        return self.persist(fail(packet, ChangeStatus::Failed, error));
                    }
                };
                if let Some(rejected) = reject_disallowed(&change_request, &packet) {
                    return self.persist(rejected);
                }
                if packet.status() == &ChangeStatus::ChecksPassed {
                    packet = packet.with_acceptance_verdict(AcceptanceVerdict::Approved);
                }
            }
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
        selected_worker: Option<&ImplementWorkerRuntime>,
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
        let mut python_runtime_checked = false;
        for command in request.acceptance().commands() {
            if !python_runtime_checked && requires_python_runtime(command.argv()) {
                match self.run_python_runtime_preflight(
                    executor,
                    workspace,
                    timeout,
                    packet.clone(),
                    commands,
                ) {
                    Ok(recorded) => commands = recorded,
                    Err(packet) => return Ok(packet),
                }
                python_runtime_checked = true;
            }
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
            .with_status(ChangeStatus::ChecksPassed))
    }

    fn run_python_runtime_preflight(
        &self,
        executor: &dyn WorkspaceExecutor,
        workspace: &ChangeWorkspace,
        timeout: u32,
        packet: ReviewPacket,
        mut commands: Vec<crate::CommandEvidence>,
    ) -> Result<Vec<crate::CommandEvidence>, ReviewPacket> {
        for argv in python_runtime_probe_commands() {
            let request =
                match RunCommandRequest::new(workspace.worktree_path().to_path_buf(), argv) {
                    Ok(value) => value.with_timeout_seconds(timeout),
                    Err(error) => {
                        return Err(fail(
                            packet.with_commands(commands),
                            ChangeStatus::ChecksFailed,
                            error,
                        ));
                    }
                };
            let result = executor.run_command(&request);
            match result {
                Ok(execution) => {
                    let evidence = execution.evidence().clone();
                    let failed = !evidence.succeeded();
                    commands.push(evidence.clone());
                    if failed {
                        return Err(fail(
                            packet.with_commands(commands),
                            ChangeStatus::ChecksFailed,
                            python_runtime_failure_message(&evidence),
                        ));
                    }
                }
                Err(error) => {
                    return Err(fail(
                        packet.with_commands(commands),
                        check_status(&error),
                        error,
                    ));
                }
            }
        }
        Ok(commands)
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

fn requires_python_runtime(argv: &[String]) -> bool {
    matches!(
        argv.first().map(String::as_str),
        Some("python3") | Some("pytest")
    )
}

fn python_runtime_probe_commands() -> Vec<Vec<String>> {
    vec![
        probe_command(&PYTHON_EXECUTABLE_PROBE),
        probe_command(&PYTHON_VERSION_PROBE),
        probe_command(&PYTEST_VERSION_PROBE),
    ]
}

fn probe_command(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|value| value.to_string()).collect()
}

fn python_runtime_failure_message(evidence: &crate::CommandEvidence) -> String {
    let detail = if !evidence.stderr().trim().is_empty() {
        evidence.stderr().trim().to_string()
    } else if !evidence.stdout().trim().is_empty() {
        evidence.stdout().trim().to_string()
    } else if evidence.timed_out() {
        "timed out".to_string()
    } else {
        format!("exit code {}", evidence.exit_code())
    };
    format!(
        "python runtime preflight failed: {} ({detail})",
        evidence.argv().join(" ")
    )
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
        .with_acceptance_verdict(AcceptanceVerdict::Rejected)
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
    use std::collections::VecDeque;
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
                selected_worker: None,
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
        let executor = FakeExecutor::new();
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
    fn records_failed_acceptance_command() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::with_outcomes(vec![FakeCommandResult::completed(1)]);
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
        let executor = FakeExecutor::new();
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
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Approved)
        );
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
        let executor = FakeExecutor::new();
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
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
        assert!(result.packet.last_error().unwrap().contains("README.md"));
        assert!(result.packet.commands().is_empty());
    }

    #[test]
    fn rejects_out_of_policy_paths_after_checks() {
        let git = FakeGit::matching("a".repeat(40))
            .with_after_paths(vec!["src/lib.rs".to_string()])
            .with_after_checks_paths(vec!["src/lib.rs".to_string(), "README.md".to_string()]);
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::new();
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
        assert_eq!(
            result.packet.acceptance_verdict(),
            Some(&AcceptanceVerdict::Rejected)
        );
        assert!(result.packet.last_error().unwrap().contains("README.md"));
        assert_eq!(result.packet.commands().len(), 1);
    }

    #[test]
    fn fails_closed_when_implementer_missing() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::new();
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
    fn python_acceptance_runs_runtime_preflight_before_command() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::new();
        let result = execute_document(
            &git,
            &manifests,
            python_document(vec![
                "python3".to_string(),
                "scripts/assert_test_fails.py".to_string(),
                "tests/test_example.py::test_red".to_string(),
            ]),
            ChangeExecutionMode::ChecksOnly,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksPassed);
        assert_eq!(executor.commands(), python_probe_and_command_argvs());
        assert_eq!(result.packet.commands().len(), 4);
    }

    #[test]
    fn python_runtime_preflight_fails_closed_when_pytest_is_missing() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::with_outcomes(vec![
            FakeCommandResult::success().with_stdout("/usr/bin/python3"),
            FakeCommandResult::success().with_stdout("Python 3.14.4"),
            FakeCommandResult::completed(1).with_stderr("/usr/bin/python3: No module named pytest"),
        ]);
        let result = execute_document(
            &git,
            &manifests,
            python_document(vec![
                "python3".to_string(),
                "scripts/assert_test_fails.py".to_string(),
                "tests/test_example.py::test_red".to_string(),
            ]),
            ChangeExecutionMode::ChecksOnly,
            Some(&executor),
            None,
        )
        .unwrap();
        assert_eq!(result.packet.status(), &ChangeStatus::ChecksFailed);
        assert_eq!(result.packet.commands().len(), 3);
        assert!(
            result
                .packet
                .last_error()
                .unwrap()
                .contains("python runtime preflight failed")
        );
        assert!(
            result
                .packet
                .last_error()
                .unwrap()
                .contains("No module named pytest")
        );
        assert_eq!(
            executor.commands(),
            vec![
                super::probe_command(&super::PYTHON_EXECUTABLE_PROBE),
                super::probe_command(&super::PYTHON_VERSION_PROBE),
                super::probe_command(&super::PYTEST_VERSION_PROBE),
            ]
        );
    }

    #[test]
    fn non_python_acceptance_does_not_run_python_preflight() {
        let git = FakeGit::matching("a".repeat(40));
        let manifests = FakeManifests::default();
        let executor = FakeExecutor::new();
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
            executor.commands(),
            vec![vec!["cargo".to_string(), "test".to_string()]]
        );
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
            selected_worker: None,
        })
    }

    fn execute_document(
        git: &FakeGit,
        manifests: &FakeManifests,
        document: ChangeRequestDocument,
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
            document,
            mode,
            selected_worker: None,
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

    fn python_document(command: Vec<String>) -> ChangeRequestDocument {
        serde_json::from_value(serde_json::json!({
            "change_id": "job-1",
            "repository": {
                "id": "adaptos",
                "registered_root": "/srv/projects/adaptos",
                "base_ref": "main",
                "base_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
            "task": "Run Python acceptance.",
            "allowed_paths": ["src/", "tests/", "scripts/"],
            "acceptance": {"commands": [command]},
            "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
        }))
        .unwrap()
    }

    fn python_probe_and_command_argvs() -> Vec<Vec<String>> {
        vec![
            super::probe_command(&super::PYTHON_EXECUTABLE_PROBE),
            super::probe_command(&super::PYTHON_VERSION_PROBE),
            super::probe_command(&super::PYTEST_VERSION_PROBE),
            vec![
                "python3".to_string(),
                "scripts/assert_test_fails.py".to_string(),
                "tests/test_example.py::test_red".to_string(),
            ],
        ]
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
        after_checks_paths: Option<Vec<String>>,
    }

    impl FakeGit {
        fn matching(sha: String) -> Self {
            Self {
                sha: GitSha::new(sha).unwrap(),
                inspect_count: Cell::new(0),
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

    #[derive(Clone)]
    struct FakeCommandResult {
        exit_code: i32,
        stdout: String,
        stderr: String,
        timed_out: bool,
    }

    impl FakeCommandResult {
        fn success() -> Self {
            Self::completed(0)
        }

        fn completed(exit_code: i32) -> Self {
            Self {
                exit_code,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
            }
        }

        fn with_stdout(mut self, stdout: &str) -> Self {
            self.stdout = stdout.to_string();
            self
        }

        fn with_stderr(mut self, stderr: &str) -> Self {
            self.stderr = stderr.to_string();
            self
        }
    }

    struct FakeExecutor {
        outcomes: RefCell<VecDeque<Result<FakeCommandResult, String>>>,
        commands: RefCell<Vec<Vec<String>>>,
    }

    impl FakeExecutor {
        fn new() -> Self {
            Self::with_outcomes(Vec::new())
        }

        fn with_outcomes(outcomes: Vec<FakeCommandResult>) -> Self {
            let queue = outcomes
                .into_iter()
                .map(Ok)
                .collect::<VecDeque<Result<FakeCommandResult, String>>>();
            Self {
                outcomes: RefCell::new(queue),
                commands: RefCell::new(Vec::new()),
            }
        }

        fn commands(&self) -> Vec<Vec<String>> {
            self.commands.borrow().clone()
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
            self.commands.borrow_mut().push(request.argv().to_vec());
            let outcome = self
                .outcomes
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(FakeCommandResult::success()))?;
            let evidence = CommandEvidence::new(request.argv().to_vec(), outcome.exit_code)
                .with_stdout(outcome.stdout)
                .with_stderr(outcome.stderr)
                .with_timed_out(outcome.timed_out);
            Ok(WorkspaceExecutionResult::new(evidence))
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
