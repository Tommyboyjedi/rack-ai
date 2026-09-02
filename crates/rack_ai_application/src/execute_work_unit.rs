use serde::Serialize;

use rack_ai_domain::AcceptanceVerdict;
use rack_ai_domain::ChangeStatus;
use rack_ai_domain::Placement;

use crate::ChangeImplementer;
use crate::ChangeManifestRepository;
use crate::CommandPolicy;
use crate::ExecuteChange;
use crate::ExecuteChangeDependencies;
use crate::ExecuteChangeRequest;
use crate::GitWorktree;
use crate::ImplementWorkerRuntime;
use crate::RepositoryRegistry;
use crate::ReviewPacket;
use crate::WorkUnitRequest;
use crate::WorkUnitRequestDocument;
use crate::WorkerExecutionProvenance;
use crate::WorkspaceExecutor;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkUnitWorkerSelection {
    runtime: ImplementWorkerRuntime,
    placement: Placement,
}

impl WorkUnitWorkerSelection {
    pub fn new(runtime: ImplementWorkerRuntime, placement: Placement) -> Self {
        Self { runtime, placement }
    }

    pub fn runtime(&self) -> &ImplementWorkerRuntime {
        &self.runtime
    }

    pub fn placement(&self) -> &Placement {
        &self.placement
    }
}

pub trait WorkUnitWorkerSelector {
    fn select(&self, request: &WorkUnitRequest) -> Result<WorkUnitWorkerSelection, String>;
}

pub struct ExecuteWorkUnit<'a> {
    registry: &'a dyn RepositoryRegistry,
    command_policy: &'a dyn CommandPolicy,
    git: &'a dyn GitWorktree,
    manifests: &'a dyn ChangeManifestRepository,
    executor: Option<&'a dyn WorkspaceExecutor>,
    implementer: Option<&'a dyn ChangeImplementer>,
    selector: &'a dyn WorkUnitWorkerSelector,
}

pub struct ExecuteWorkUnitDependencies<'a> {
    pub registry: &'a dyn RepositoryRegistry,
    pub command_policy: &'a dyn CommandPolicy,
    pub git: &'a dyn GitWorktree,
    pub manifests: &'a dyn ChangeManifestRepository,
    pub executor: Option<&'a dyn WorkspaceExecutor>,
    pub implementer: Option<&'a dyn ChangeImplementer>,
    pub selector: &'a dyn WorkUnitWorkerSelector,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExecuteWorkUnitResult {
    pub workload_id: String,
    pub work_unit_id: String,
    pub change_id: String,
    pub selected_worker_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_provenance: Option<WorkerExecutionProvenance>,
    pub placement: Placement,
    pub status: ChangeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance_verdict: Option<AcceptanceVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_revision: Option<String>,
    pub branch: String,
    pub worktree_path: String,
    pub packet_path: String,
}

impl<'a> ExecuteWorkUnit<'a> {
    pub fn new(dependencies: ExecuteWorkUnitDependencies<'a>) -> Self {
        Self {
            registry: dependencies.registry,
            command_policy: dependencies.command_policy,
            git: dependencies.git,
            manifests: dependencies.manifests,
            executor: dependencies.executor,
            implementer: dependencies.implementer,
            selector: dependencies.selector,
        }
    }

    pub fn execute(
        &self,
        document: WorkUnitRequestDocument,
    ) -> Result<ExecuteWorkUnitResult, String> {
        let request = WorkUnitRequest::from_document(document)?;
        let selection = self.selector.select(&request)?;
        let change = ExecuteChange::new(ExecuteChangeDependencies {
            registry: self.registry,
            command_policy: self.command_policy,
            git: self.git,
            manifests: self.manifests,
            executor: self.executor,
            implementer: self.implementer,
        })
        .execute(ExecuteChangeRequest {
            document: request.to_change_request_document(),
            mode: crate::ChangeExecutionMode::ImplementAndVerify,
            selected_worker: Some(selection.runtime().clone()),
        })?;
        Ok(build_result(
            &request,
            selection.runtime(),
            selection.placement(),
            &change.packet,
            change.packet_path,
        ))
    }
}

fn build_result(
    request: &WorkUnitRequest,
    runtime: &ImplementWorkerRuntime,
    placement: &Placement,
    packet: &ReviewPacket,
    packet_path: String,
) -> ExecuteWorkUnitResult {
    ExecuteWorkUnitResult {
        workload_id: request.workload_id().value().to_string(),
        work_unit_id: request.work_unit_id().value().to_string(),
        change_id: request.change_id(),
        selected_worker_id: runtime.worker_id().to_string(),
        worker_provenance: packet.worker_provenance().cloned(),
        placement: placement.clone(),
        status: packet.status().clone(),
        acceptance_verdict: packet.acceptance_verdict().cloned(),
        accepted_revision: accepted_revision(packet),
        branch: packet.branch().to_string(),
        worktree_path: packet.worktree_path().to_string(),
        packet_path,
    }
}

fn accepted_revision(packet: &ReviewPacket) -> Option<String> {
    if packet.acceptance_verdict() == Some(&AcceptanceVerdict::Approved) {
        Some(packet.head_sha().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_domain::AcceptanceVerdict;
    use rack_ai_domain::ChangeStatus;
    use rack_ai_domain::GitSha;
    use rack_ai_domain::Placement;
    use rack_ai_domain::RepositoryId;

    use super::ExecuteWorkUnit;
    use super::ExecuteWorkUnitDependencies;
    use super::WorkUnitWorkerSelection;
    use super::WorkUnitWorkerSelector;
    use crate::ApprovedCommandPolicy;
    use crate::ChangeManifestRepository;
    use crate::CommandEvidence;
    use crate::CreateChangeWorktreeRequest;
    use crate::ExecutorConfig;
    use crate::GitEvidence;
    use crate::GitWorktree;
    use crate::ImplementWorkerRuntime;
    use crate::InspectChangeWorktreeRequest;
    use crate::ReadFileRequest;
    use crate::RegisteredRepository;
    use crate::RepositoryRegistry;
    use crate::ResolveGitShaRequest;
    use crate::ReviewPacket;
    use crate::RunCommandRequest;
    use crate::ScriptedAttempt;
    use crate::ScriptedChangeImplementer;
    use crate::ScriptedWrite;
    use crate::WorkUnitRequestDocument;
    use crate::WorkspaceExecutionResult;
    use crate::WorkspaceExecutor;
    use crate::WorkspaceRoot;
    use crate::WriteFileRequest;

    #[test]
    fn executes_work_unit_and_returns_structured_result() {
        let fixture = Fixture::new();
        let git = FixtureGit::new(&fixture.root, vec!["src/lib.rs".to_string()]);
        let manifests = FixtureManifests::default();
        let executor = FixtureExecutor::default();
        let implementer = ScriptedChangeImplementer::new(
            &executor,
            vec![ScriptedAttempt {
                match_worker: Some("local-coder".to_string()),
                writes: vec![ScriptedWrite {
                    path: "src/lib.rs".to_string(),
                    content: "pub fn tiny() -> &'static str { \"ok\" }\n".to_string(),
                }],
                output: "suggested different checks".to_string(),
                error: None,
                protocol_error: None,
                executor_kind: Some("jcode-direct".to_string()),
            }],
        );
        let selector = FixedSelector::new(
            "local-coder",
            Placement::new(
                vec!["local-coder".to_string()],
                vec!["gpu-2060".to_string()],
            )
            .with_models(vec!["eqaq-v2-local-coder".to_string()])
            .with_backends(vec!["jcode".to_string()]),
        );
        let result = ExecuteWorkUnit::new(ExecuteWorkUnitDependencies {
            registry: &fixture,
            command_policy: &ApprovedCommandPolicy::default(),
            git: &git,
            manifests: &manifests,
            executor: Some(&executor),
            implementer: Some(&implementer),
            selector: &selector,
        })
        .execute(sample_document())
        .unwrap();
        assert_eq!(result.workload_id, "adaptos");
        assert_eq!(result.work_unit_id, "adaptos-001");
        assert_eq!(result.selected_worker_id, "local-coder");
        assert_eq!(
            result.worker_provenance.as_ref().unwrap().worker_id,
            "local-coder"
        );
        assert_eq!(result.status, ChangeStatus::ChecksPassed);
        assert_eq!(result.acceptance_verdict, Some(AcceptanceVerdict::Approved));
        assert_eq!(result.accepted_revision, Some("b".repeat(40)));
        assert_eq!(
            executor.seen_commands(),
            vec![vec![
                "cargo".to_string(),
                "test".to_string(),
                "save_single_open_ticket".to_string()
            ]]
        );
        assert_eq!(implementer.seen_workers(), vec!["local-coder".to_string()]);
    }

    #[test]
    fn rejects_not_ready_work_unit_before_execution() {
        let fixture = Fixture::new();
        let git = FixtureGit::new(&fixture.root, vec!["src/lib.rs".to_string()]);
        let manifests = FixtureManifests::default();
        let executor = FixtureExecutor::default();
        let implementer = ScriptedChangeImplementer::new(&executor, vec![]);
        let selector = FixedSelector::new(
            "local-coder",
            Placement::new(
                vec!["local-coder".to_string()],
                vec!["gpu-2060".to_string()],
            ),
        );
        let error = ExecuteWorkUnit::new(ExecuteWorkUnitDependencies {
            registry: &fixture,
            command_policy: &ApprovedCommandPolicy::default(),
            git: &git,
            manifests: &manifests,
            executor: Some(&executor),
            implementer: Some(&implementer),
            selector: &selector,
        })
        .execute(
            serde_json::from_value(serde_json::json!({
                "version": "rack-ai/work-unit/v1",
                "workload": {"id": "adaptos", "kind": "application-development"},
                "repository": {"id": "adaptos", "base_ref": "main"},
                "work_unit": {
                    "id": "adaptos-001",
                    "objective": "Implement a bounded feature.",
                    "allowed_paths": ["src/"],
                    "acceptance": {"commands": [["cargo", "test"]]},
                    "readiness": {"ready": false},
                    "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
                }
            }))
            .unwrap(),
        )
        .unwrap_err();
        assert!(error.contains("not marked ready"));
    }

    #[derive(Default)]
    struct FixtureExecutor {
        commands: RefCell<Vec<Vec<String>>>,
    }

    impl FixtureExecutor {
        fn seen_commands(&self) -> Vec<Vec<String>> {
            self.commands.borrow().clone()
        }
    }

    impl WorkspaceExecutor for FixtureExecutor {
        fn write_file(
            &self,
            request: &WriteFileRequest,
        ) -> Result<WorkspaceExecutionResult, String> {
            let path = request.worktree_path().join(request.path().relative());
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(&path, request.content()).map_err(|error| error.to_string())?;
            Ok(WorkspaceExecutionResult::new(CommandEvidence::new(
                vec!["write".to_string()],
                0,
            )))
        }

        fn read_file(&self, request: &ReadFileRequest) -> Result<WorkspaceExecutionResult, String> {
            let path = request.worktree_path().join(request.path().relative());
            let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
            Ok(
                WorkspaceExecutionResult::new(CommandEvidence::new(vec!["read".to_string()], 0))
                    .with_content(content),
            )
        }

        fn run_command(
            &self,
            request: &RunCommandRequest,
        ) -> Result<WorkspaceExecutionResult, String> {
            self.commands.borrow_mut().push(request.argv().to_vec());
            Ok(WorkspaceExecutionResult::new(
                CommandEvidence::new(request.argv().to_vec(), 0).with_stdout("ok\n".to_string()),
            ))
        }
    }

    struct FixedSelector {
        selection: WorkUnitWorkerSelection,
    }

    impl FixedSelector {
        fn new(worker_id: &str, placement: Placement) -> Self {
            Self {
                selection: WorkUnitWorkerSelection::new(
                    ImplementWorkerRuntime::new(
                        worker_id.to_string(),
                        "/home/tomp/.local/bin/jcode".to_string(),
                        worker_id.to_string(),
                        worker_id.to_string(),
                        "http://127.0.0.1:8018/v1".to_string(),
                    )
                    .with_worker_provenance(crate::WorkerExecutionProvenance {
                        worker_id: worker_id.to_string(),
                        worker_role: "implementer-tester".to_string(),
                        worker_kind: "jcode".to_string(),
                        model_id: "eqaq-v2-local-coder".to_string(),
                        provider_profile: worker_id.to_string(),
                        resource_id: "gpu-2060".to_string(),
                        backend: "jcode".to_string(),
                        tool_profile: Some("minimal".to_string()),
                    }),
                    placement,
                ),
            }
        }
    }

    impl WorkUnitWorkerSelector for FixedSelector {
        fn select(
            &self,
            _request: &crate::WorkUnitRequest,
        ) -> Result<WorkUnitWorkerSelection, String> {
            Ok(self.selection.clone())
        }
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("rack-ai-work-unit-{nanos}"));
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("src/lib.rs"), "pub fn baseline() {}\n").unwrap();
            Self { root }
        }
    }

    impl RepositoryRegistry for Fixture {
        fn workspace_root(&self) -> Result<WorkspaceRoot, String> {
            WorkspaceRoot::new(self.root.join("workspaces"))
        }

        fn executor_config(&self) -> Result<ExecutorConfig, String> {
            ExecutorConfig::podman("rust:bookworm".to_string())
        }

        fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
            RegisteredRepository::new(id.clone(), self.root.clone())
        }
    }

    struct FixtureGit {
        root: PathBuf,
        changed_paths: Vec<String>,
        sha: GitSha,
        commit_sha: GitSha,
        inspect_count: RefCell<usize>,
    }

    impl FixtureGit {
        fn new(root: &PathBuf, changed_paths: Vec<String>) -> Self {
            Self {
                root: root.clone(),
                changed_paths,
                sha: GitSha::new("a".repeat(40)).unwrap(),
                commit_sha: GitSha::new("b".repeat(40)).unwrap(),
                inspect_count: RefCell::new(0),
            }
        }
    }

    impl GitWorktree for FixtureGit {
        fn resolve_sha(&self, _request: &ResolveGitShaRequest) -> Result<GitSha, String> {
            Ok(self.sha.clone())
        }

        fn create(
            &self,
            request: &CreateChangeWorktreeRequest,
        ) -> Result<crate::ChangeWorkspace, String> {
            let worktree = request.worktree_path().to_path_buf();
            fs::create_dir_all(worktree.join("src")).map_err(|error| error.to_string())?;
            fs::write(
                worktree.join("src/lib.rs"),
                fs::read_to_string(self.root.join("src/lib.rs")).unwrap(),
            )
            .map_err(|error| error.to_string())?;
            Ok(crate::ChangeWorkspace::new(
                rack_ai_domain::ChangeId::new("work-unit".to_string()).unwrap(),
                worktree,
            )
            .with_branch_name(request.branch_name().to_string())
            .with_base_sha(self.sha.clone()))
        }

        fn inspect(&self, _request: &InspectChangeWorktreeRequest) -> Result<GitEvidence, String> {
            let mut count = self.inspect_count.borrow_mut();
            *count += 1;
            let paths = if *count == 1 {
                Vec::new()
            } else {
                self.changed_paths.clone()
            };
            Ok(GitEvidence::new(self.sha.clone(), String::new()).with_changed_paths(paths))
        }

        fn commit_local(&self, _request: &crate::CampaignCommitRequest) -> Result<GitSha, String> {
            Ok(self.commit_sha.clone())
        }
    }

    #[derive(Default)]
    struct FixtureManifests {
        counter: RefCell<u32>,
    }

    impl ChangeManifestRepository for FixtureManifests {
        fn save(&self, packet: &ReviewPacket) -> Result<String, String> {
            let mut counter = self.counter.borrow_mut();
            *counter += 1;
            Ok(format!(
                "/tmp/packet-{}-{}.json",
                packet.change_id(),
                *counter
            ))
        }
    }

    fn sample_document() -> WorkUnitRequestDocument {
        serde_json::from_value(serde_json::json!({
            "version": "rack-ai/work-unit/v1",
            "workload": {"id": "adaptos", "kind": "application-development"},
            "repository": {"id": "adaptos", "base_ref": "main", "base_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            "work_unit": {
                "id": "adaptos-001",
                "objective": "Implement TicketStore::save(path) for one open ticket.",
                "allowed_paths": ["src/lib.rs"],
                "acceptance": {
                    "commands": [["cargo", "test", "save_single_open_ticket"]],
                    "required_artifacts": ["src/lib.rs"]
                },
                "limits": {"max_implementation_attempts": 2, "timeout_seconds": 900}
            }
        }))
        .unwrap()
    }
}
