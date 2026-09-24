//! Acceptance uses the same persisted invocation cancellation fence as model dispatch.
use rack_ai_application::implement_worker_runtime::ReservedAccess;
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Deserialize)]
struct Document {
    data: Data,
}
#[derive(Deserialize)]
struct Data {
    invocations: BTreeMap<String, Invocation>,
    demands: BTreeMap<String, Demand>,
}
#[derive(Deserialize)]
struct Invocation {
    state: State,
    cancellation: Option<serde_json::Value>,
    execution_deadline: Option<u64>,
    request: Request,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum State {
    #[serde(alias = "accepted")]
    Queued,
    #[serde(alias = "started")]
    Running,
    Completed,
    Cancelled,
    Failed,
    Expired,
    Uncertain,
}
#[derive(Deserialize)]
struct Request {
    reservation_id: String,
}
#[derive(Deserialize)]
struct Demand {
    released: bool,
    deadline: u64,
}
pub fn check(access: &ReservedAccess) -> Result<(), String> {
    let bytes =
        std::fs::read(access.authority_root.join("managed.json")).map_err(|e| e.to_string())?;
    let doc: Document = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let i = doc
        .data
        .invocations
        .get(&access.invocation_id)
        .ok_or("reserved work missing")?;
    let d = doc
        .data
        .demands
        .get(&i.request.reservation_id)
        .ok_or("reservation missing")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    if i.state != State::Running
        || i.cancellation.is_some()
        || d.released
        || d.deadline <= now
        || i.execution_deadline.is_none_or(|deadline| deadline <= now)
    {
        return Err("reserved work cancelled, expired or uncertain".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-reserved-execution-{name}-{unique}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn future() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600
    }

    fn access(root: PathBuf, invocation_id: &str) -> ReservedAccess {
        ReservedAccess {
            endpoint: "http://127.0.0.1:1/v1".into(),
            invocation_id: invocation_id.into(),
            authority_root: root,
        }
    }

    fn write_managed(root: &PathBuf, value: serde_json::Value) {
        fs::write(
            root.join("managed.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn retained_failed_invocation_does_not_poison_running_reserved_check() {
        let root = temp_root("retained-failed");
        write_managed(
            &root,
            json!({
                "data": {
                    "invocations": {
                        "running": {
                            "state": "running",
                            "cancellation": null,
                            "execution_deadline": future(),
                            "request": {"reservation_id": "reservation"}
                        },
                        "retained-failed": {
                            "state": "failed",
                            "cancellation": null,
                            "execution_deadline": future(),
                            "request": {"reservation_id": "reservation"}
                        }
                    },
                    "demands": {
                        "reservation": {
                            "released": false,
                            "deadline": future()
                        }
                    }
                }
            }),
        );

        check(&access(root.clone(), "running")).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_reserved_invocation_is_rejected_as_not_running() {
        let root = temp_root("failed-target");
        write_managed(
            &root,
            json!({
                "data": {
                    "invocations": {
                        "failed-target": {
                            "state": "failed",
                            "cancellation": null,
                            "execution_deadline": future(),
                            "request": {"reservation_id": "reservation"}
                        }
                    },
                    "demands": {
                        "reservation": {
                            "released": false,
                            "deadline": future()
                        }
                    }
                }
            }),
        );

        assert_eq!(
            check(&access(root.clone(), "failed-target")).unwrap_err(),
            "reserved work cancelled, expired or uncertain"
        );
        let _ = fs::remove_dir_all(root);
    }
}
