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
