use super::*;

#[test]
fn long_inherited_tmpdir_does_not_break_socket_launch() {
    const CHILD: &str = "RACK_AI_SOCKET_REGRESSION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let workdir = std::env::temp_dir();
        let runtime = rack_ai_application::ImplementWorkerRuntime::new(
            "fixture".into(),
            workdir.join("fixture.sh").display().to_string(),
            "fixture".into(),
            "fixture".into(),
            "http://127.0.0.1:9/v1".into(),
        );
        let output = JCodeProcessRunner::run(&runtime, "no model", &workdir, 5, true).unwrap();
        print!("{}", output.stdout());
        return;
    }
    let root = temp_root();
    let long_tmp = root.join(format!(
        "{}--{}--submission-8484878532335806390",
        "opaque-work-".repeat(6),
        "opaque-submission-".repeat(5)
    ));
    fs::create_dir_all(&long_tmp).unwrap();
    let legacy_socket = long_tmp.join("rack-ai-jcode-run-1788625894000000000-0/selected-vllm.sock");
    assert!(legacy_socket.as_os_str().len() >= 108);
    assert_eq!(
        UnixListener::bind(&legacy_socket).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    let mut children = Vec::new();
    for suffix in ["first", "second"] {
        let workdir = long_tmp.join(suffix);
        fs::create_dir_all(&workdir).unwrap();
        write_executable(
            &workdir.join("fixture.sh"),
            "#!/bin/bash\nprintf 'RUNTIME_ROOT=%s\\n' \"${HOME%/home}\"\n",
        )
        .unwrap();
        children.push(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "jcode_process_runner::socket_path_tests::long_inherited_tmpdir_does_not_break_socket_launch", "--nocapture"])
            .env(CHILD, "1").env("TMPDIR", &workdir).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap());
    }
    let mut runtime_paths = Vec::new();
    for child in children {
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let path = PathBuf::from(
            stdout
                .lines()
                .find_map(|line| line.strip_prefix("RUNTIME_ROOT="))
                .unwrap(),
        );
        assert_eq!(
            path.join(SOCKET_NAME).as_os_str().len(),
            runtime_root::MAX_SOCKET_PATH_BYTES
        );
        assert!(!path.exists());
        runtime_paths.push(path);
    }
    fs::remove_dir_all(&root).unwrap();
    assert_ne!(runtime_paths[0], runtime_paths[1]);
}

#[test]
fn concurrent_runtime_roots_are_private_bounded_and_exclusive() {
    use std::os::unix::fs::PermissionsExt;
    let first = JCodeRuntimeRoot::create().unwrap();
    let second = JCodeRuntimeRoot::create().unwrap();
    let paths = [first.path().to_owned(), second.path().to_owned()];
    assert_ne!(paths[0], paths[1]);
    for root in &paths {
        let socket = root.join(SOCKET_NAME);
        assert_eq!(
            socket.as_os_str().len(),
            runtime_root::MAX_SOCKET_PATH_BYTES
        );
        assert!(runtime_root::MAX_SOCKET_PATH_BYTES < 108);
        assert_eq!(
            fs::metadata(root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let listener = UnixListener::bind(socket).unwrap();
        drop(listener);
    }
    drop((first, second));
    assert!(paths.iter().all(|path| !path.exists()));
}

#[test]
fn socket_and_home_cleanup_on_success_failure_timeout_spawn_and_setup_errors() {
    for outcome in ["success", "failure", "timeout", "spawn", "setup"] {
        let root = JCodeRuntimeRoot::create().unwrap();
        let path = root.path().to_owned();
        let script = path.join("fixture.sh");
        let action = match outcome {
            "failure" => "exit 42",
            "timeout" => "sleep 30",
            _ => "exit 0",
        };
        write_executable(&script, &format!(
            "#!/bin/bash\ntest -S \"$HOME/../selected-vllm.sock\" || exit 91\nprintf 'SOCKET_READY\\n'\n{action}\n"
        )).unwrap();
        let endpoint = if outcome == "setup" {
            "http://invalid.example:9/v1"
        } else {
            "http://127.0.0.1:9/v1"
        };
        let runtime = ImplementWorkerRuntime::new(
            "fixture".into(),
            script.display().to_string(),
            "fixture".into(),
            "fixture".into(),
            endpoint.into(),
        );
        let workdir = if outcome == "spawn" {
            path.join("missing")
        } else {
            path.clone()
        };
        let result = run_with_root(&runtime, "fixture only", &workdir, 1, true, &path, None);
        assert!(!path.join(SOCKET_NAME).exists(), "socket leaked: {outcome}");
        match outcome {
            "success" => assert!(result.unwrap().stdout().contains("SOCKET_READY")),
            "failure" | "timeout" => {
                let error = result.unwrap_err();
                assert!(error.stdout().contains("SOCKET_READY"));
                assert!(error.message().contains(if outcome == "timeout" {
                    "wall-clock timeout exceeded"
                } else {
                    "jcode exited unsuccessfully"
                }));
            }
            _ => {
                assert!(result.is_err());
            }
        }
        drop(root);
        assert!(!path.exists(), "runtime root leaked: {outcome}");
    }
}

#[test]
fn full_durable_identities_survive_short_runtime_execution() {
    use rack_ai_application::{
        ChangeManifestRepository, GenericCapability, GenericPriority, GenericRoutingHeader,
        GenericWorkerSelectionDecision, ReviewPacket, WorkerExecutionProvenance,
    };
    let evidence = temp_root();
    let change_id = format!(
        "{}--{}--submission-8484878532335806390",
        "opaque-work-".repeat(6),
        "opaque-submission-".repeat(5)
    );
    let header = GenericRoutingHeader::new(
        "neutral".into(),
        "full-work-".repeat(20),
        "full-submission-".repeat(20),
        "full-idempotency-".repeat(20),
        vec![GenericCapability::Coding],
        GenericPriority::Medium,
    )
    .unwrap();
    let mut decision = GenericWorkerSelectionDecision::new(
        &header,
        rack_ai_domain::WorkUnitComplexity::Small,
        false,
    );
    decision.selected_worker_id = Some("fixture".into());
    decision.eligible_worker_ids = vec!["fixture".into()];
    let provenance = WorkerExecutionProvenance {
        worker_id: "fixture".into(),
        worker_role: "generic".into(),
        worker_kind: "jcode".into(),
        model_id: "fixture".into(),
        provider_profile: "fixture".into(),
        resource_id: "fixture-resource".into(),
        backend: "jcode".into(),
        tool_profile: None,
    };
    let workdir = evidence.join(&change_id).join("repo");
    fs::create_dir_all(&workdir).unwrap();
    let runtime = ImplementWorkerRuntime::new(
        "fixture".into(),
        "/bin/true".into(),
        "fixture".into(),
        "fixture".into(),
        "http://127.0.0.1:9/v1".into(),
    )
    .with_worker_provenance(provenance.clone());
    JCodeProcessRunner::run(&runtime, "fixture only", &workdir, 5, true).unwrap();
    assert_eq!(runtime.worker_provenance(), Some(&provenance));
    let packet = ReviewPacket::new(change_id.clone(), "neutral".into())
        .with_selection_decision(decision.clone())
        .with_worker_provenance(provenance.clone());
    let repository = crate::FileSystemChangeManifestRepository::new(crate::RepositoryPaths::new(
        evidence.clone(),
    ));
    let path = repository.save(&packet).unwrap();
    assert_eq!(
        Path::new(&path),
        evidence
            .join("state/changes")
            .join(&change_id)
            .join("review-packet.json")
    );
    let restored: ReviewPacket = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(restored.change_id(), change_id);
    assert_eq!(restored.selection_decision(), Some(&decision));
    assert_eq!(restored.worker_provenance(), Some(&provenance));
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(packet).unwrap()
    );
    assert!(repository.has_idempotent_submission(&header).unwrap());
    let mut other = header.clone();
    other.submission_id.push_str("-distinct");
    assert!(!repository.has_idempotent_submission(&other).unwrap());
    fs::remove_dir_all(evidence).unwrap();
}
