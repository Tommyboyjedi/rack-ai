use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rack_ai_application::ApprovedCommandPolicy;
use rack_ai_application::ChangeImplementer;
use rack_ai_application::ExecuteWorkUnit;
use rack_ai_application::ExecuteWorkUnitDependencies;
use rack_ai_application::GitWorktree;
use rack_ai_application::ImplementChangeRequest;
use rack_ai_application::ImplementChangeResult;
use rack_ai_application::ImplementWorkerRuntime;
use rack_ai_application::RegisteredRepository;
use rack_ai_application::RepositoryRegistry;
use rack_ai_application::ResolveGitShaRequest;
use rack_ai_application::RunCommandRequest;
use rack_ai_application::WorkUnitRequest;
use rack_ai_application::WorkUnitRequestDocument;
use rack_ai_application::WorkUnitWorkerSelection;
use rack_ai_application::WorkUnitWorkerSelector;
use rack_ai_application::WorkspaceExecutionResult;
use rack_ai_application::WorkspaceExecutor;
use rack_ai_application::WorkspaceRoot;
use rack_ai_domain::AcceptanceVerdict;
use rack_ai_domain::GitRef;
use rack_ai_domain::Placement;
use rack_ai_domain::RepositoryId;

use crate::FileSystemChangeManifestRepository;
use crate::GitCommand;
use crate::GitCommandWorktree;
use crate::RepositoryPaths;

#[test]
fn approved_work_units_produce_trusted_progression_chain() {
    let fixture = GitFixture::new();
    let git = GitCommandWorktree;
    let manifests =
        FileSystemChangeManifestRepository::new(RepositoryPaths::new(fixture.state_root.clone()));
    let executor = AcceptingExecutor::default();
    let implementer = AssertingImplementer::new(vec![
        ImplementSpec::new("original\n", "alpha\n"),
        ImplementSpec::new("alpha\n", "alpha\nbeta\n"),
    ]);
    let policy = ApprovedCommandPolicy::default();
    let selector = FixedSelector::new(
        "local-coder",
        Placement::new(
            vec!["local-coder".to_string()],
            vec!["gpu-2060".to_string()],
        )
        .with_models(vec!["eqaq-v2-local-coder".to_string()])
        .with_backends(vec!["jcode".to_string()]),
    );
    let base_sha = git
        .resolve_sha(&ResolveGitShaRequest::new(
            fixture.repo_root.clone(),
            GitRef::new("main".to_string()).unwrap(),
        ))
        .unwrap()
        .value()
        .to_string();
    let service = ExecuteWorkUnit::new(ExecuteWorkUnitDependencies {
        registry: &fixture,
        command_policy: &policy,
        git: &git,
        manifests: &manifests,
        executor: Some(&executor),
        implementer: Some(&implementer),
        selector: &selector,
    });

    let first = service
        .execute(sample_document("wu-a", &base_sha, "Implement alpha."))
        .unwrap();
    let s1 = first.accepted_head_sha.clone().unwrap();
    assert_eq!(first.acceptance_verdict, Some(AcceptanceVerdict::Approved));
    assert_ne!(s1, base_sha);
    GitCommand::run(
        &fixture.repo_root,
        &["cat-file", "-e", &format!("{s1}^{{commit}}")],
    )
    .unwrap();
    assert_eq!(
        GitCommand::run(&fixture.repo_root, &["show", &format!("{s1}:src/lib.rs")]).unwrap(),
        "alpha"
    );
    let packet_one = fs::read_to_string(&first.packet_path).unwrap();
    assert!(packet_one.contains(&format!("\"base_sha\": \"{base_sha}\"")));
    assert!(packet_one.contains(&format!("\"accepted_head_sha\": \"{s1}\"")));

    let second = service
        .execute(sample_document("wu-b", &s1, "Implement beta."))
        .unwrap();
    let s2 = second.accepted_head_sha.clone().unwrap();
    assert_eq!(second.acceptance_verdict, Some(AcceptanceVerdict::Approved));
    assert_ne!(s2, s1);
    GitCommand::run(
        &fixture.repo_root,
        &["cat-file", "-e", &format!("{s2}^{{commit}}")],
    )
    .unwrap();
    assert_eq!(
        GitCommand::run(&fixture.repo_root, &["show", &format!("{s2}:src/lib.rs")]).unwrap(),
        "alpha\nbeta"
    );
    let packet_two = fs::read_to_string(&second.packet_path).unwrap();
    assert!(packet_two.contains(&format!("\"base_sha\": \"{s1}\"")));
    assert!(packet_two.contains(&format!("\"accepted_head_sha\": \"{s2}\"")));
    assert_eq!(
        executor.seen_commands(),
        vec![
            vec![
                "cargo".to_string(),
                "test".to_string(),
                "save_single_open_ticket".to_string(),
            ],
            vec![
                "cargo".to_string(),
                "test".to_string(),
                "save_single_open_ticket".to_string(),
            ],
        ]
    );
}

struct GitFixture {
    repo_root: PathBuf,
    state_root: PathBuf,
}

impl GitFixture {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-work-unit-git-{nanos}"));
        let repo_root = root.join("repo");
        fs::create_dir_all(repo_root.join("src")).unwrap();
        fs::write(repo_root.join("src/lib.rs"), "original\n").unwrap();
        let status = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&repo_root)
            .status()
            .unwrap();
        assert!(status.success());
        GitCommand::run(&repo_root, &["config", "user.email", "test@example.com"]).unwrap();
        GitCommand::run(&repo_root, &["config", "user.name", "test"]).unwrap();
        GitCommand::run(&repo_root, &["add", "."]).unwrap();
        GitCommand::run(&repo_root, &["commit", "-m", "init"]).unwrap();
        let state_root = root.join("rack");
        fs::create_dir_all(&state_root).unwrap();
        Self {
            repo_root,
            state_root,
        }
    }
}

impl RepositoryRegistry for GitFixture {
    fn workspace_root(&self) -> Result<WorkspaceRoot, String> {
        WorkspaceRoot::new(self.state_root.join("workspaces"))
    }

    fn executor_config(&self) -> Result<rack_ai_application::ExecutorConfig, String> {
        rack_ai_application::ExecutorConfig::podman("rust:bookworm".to_string())
    }

    fn find(&self, id: &RepositoryId) -> Result<RegisteredRepository, String> {
        RegisteredRepository::new(id.clone(), self.repo_root.clone())
    }
}

#[derive(Default)]
struct AcceptingExecutor {
    commands: RefCell<Vec<Vec<String>>>,
}

impl AcceptingExecutor {
    fn seen_commands(&self) -> Vec<Vec<String>> {
        self.commands.borrow().clone()
    }
}

impl WorkspaceExecutor for AcceptingExecutor {
    fn write_file(
        &self,
        request: &rack_ai_application::WriteFileRequest,
    ) -> Result<WorkspaceExecutionResult, String> {
        let path = request.worktree_path().join(request.path().relative());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(path, request.content()).map_err(|error| error.to_string())?;
        Ok(WorkspaceExecutionResult::new(
            rack_ai_application::CommandEvidence::new(vec!["write".to_string()], 0),
        ))
    }

    fn read_file(
        &self,
        request: &rack_ai_application::ReadFileRequest,
    ) -> Result<WorkspaceExecutionResult, String> {
        let path = request.worktree_path().join(request.path().relative());
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        Ok(
            WorkspaceExecutionResult::new(rack_ai_application::CommandEvidence::new(
                vec!["read".to_string()],
                0,
            ))
            .with_content(content),
        )
    }

    fn run_command(&self, request: &RunCommandRequest) -> Result<WorkspaceExecutionResult, String> {
        self.commands.borrow_mut().push(request.argv().to_vec());
        Ok(WorkspaceExecutionResult::new(
            rack_ai_application::CommandEvidence::new(request.argv().to_vec(), 0)
                .with_stdout("ok\n".to_string()),
        ))
    }
}

struct ImplementSpec {
    expected: String,
    next: String,
}

impl ImplementSpec {
    fn new(expected: &str, next: &str) -> Self {
        Self {
            expected: expected.to_string(),
            next: next.to_string(),
        }
    }
}

struct AssertingImplementer {
    specs: RefCell<Vec<ImplementSpec>>,
}

impl AssertingImplementer {
    fn new(specs: Vec<ImplementSpec>) -> Self {
        Self {
            specs: RefCell::new(specs),
        }
    }
}

impl ChangeImplementer for AssertingImplementer {
    fn implement(&self, request: &ImplementChangeRequest) -> Result<ImplementChangeResult, String> {
        let spec = self.specs.borrow_mut().remove(0);
        let path = request.worktree_path().join("src/lib.rs");
        let current = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        assert_eq!(current, spec.expected);
        fs::write(path, spec.next).map_err(|error| error.to_string())?;
        Ok(ImplementChangeResult::new("COMPLETE".to_string()))
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
                ),
                placement,
            ),
        }
    }
}

impl WorkUnitWorkerSelector for FixedSelector {
    fn select(&self, _request: &WorkUnitRequest) -> Result<WorkUnitWorkerSelection, String> {
        Ok(self.selection.clone())
    }
}

fn sample_document(work_unit_id: &str, base_sha: &str, objective: &str) -> WorkUnitRequestDocument {
    serde_json::from_value(serde_json::json!({
        "version": "rack-ai/work-unit/v1",
        "workload": {"id": "adaptos", "kind": "application-development"},
        "repository": {"id": "adaptos", "base_ref": "main", "base_sha": base_sha},
        "work_unit": {
            "id": work_unit_id,
            "objective": objective,
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
