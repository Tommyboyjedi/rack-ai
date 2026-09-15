//! Each caller runs in a child test process so its authority environment cannot race other tests.
use std::{fs, net::TcpListener, process::Command};
pub fn exercise(test: &str, assertion: impl FnOnce(String)) {
    if let Ok(endpoint) = std::env::var("RACK_AI_GATE_PROBE_ENDPOINT") {
        assertion(endpoint);
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "rack-gate-{}",
        crate::resource_reservations::new_identity().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    fs::write(root.join("managed.json"), serde_json::json!({"data":{"demands":{"owner":{"profile":{"endpoint":endpoint},"state":"ready"}}}}).to_string()).unwrap();
    let output = Command::new("timeout")
        .args(["15"])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("RACK_AI_RESOURCE_ROOT", &root)
        .env("RACK_AI_GATE_PROBE_ENDPOINT", endpoint)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        listener.accept().is_err(),
        "managed raw endpoint received a forbidden connection"
    );
    fs::remove_dir_all(root).unwrap();
}
