//! Bounded private documents; the authority snapshot alone decides visibility.
use crate::{
    interaction_diagnostics::{CallIdentity, Limits, METADATA_BYTES},
    service::Service,
    types::digest,
};
use rack_ai_application::durable_file::atomic_write_private;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub const PREFIX: &str = "mi1";
const LEASE_FILE: &str = "lease.json";
pub const MAX_DOCUMENT_BYTES: u64 = 2 * 1024 * 1024 + METADATA_BYTES;

#[derive(Deserialize, Serialize)]
pub struct Lease {
    pub owner: String,
    pub reservation_id: String,
    pub limits: Limits,
    pub omitted_interactions: u64,
    pub cleanup_pending: bool,
}
#[derive(Deserialize, Serialize)]
pub struct Start {
    pub identity: CallIdentity,
    pub artifact_id: String,
    pub dispatched_ms: u64,
    pub request: Content,
    pub response_bound: u64,
    pub charged_content_bytes: u64,
}
#[derive(Deserialize, Serialize)]
pub struct Content {
    pub body: Option<Value>,
    pub text_prefix: Option<String>,
    pub original_bytes: u64,
    pub truncated: bool,
    pub unavailable_reason: Option<String>,
}
impl Content {
    pub fn bounded(value: Value, bound: u64) -> Result<Self, String> {
        let bytes = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        let truncated = bytes.len() as u64 > bound;
        let mut end = usize::try_from(bound)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        while !bytes.is_char_boundary(end) {
            end -= 1;
        }
        let mut content = Self {
            body: (!truncated).then_some(value),
            text_prefix: (truncated && bound > 0).then(|| bytes[..end].to_string()),
            original_bytes: bytes.len() as u64,
            truncated,
            unavailable_reason: (bound == 0).then(|| "reservation_content_limit".into()),
        };
        while serde_json::to_vec(&content)
            .map_err(|e| e.to_string())?
            .len() as u64
            > bound + 512
        {
            let Some(prefix) = content.text_prefix.as_mut() else {
                break;
            };
            let mut end = prefix.len() / 2;
            while !prefix.is_char_boundary(end) {
                end -= 1;
            }
            prefix.truncate(end);
        }
        Ok(content)
    }
}
#[derive(Deserialize, Serialize)]
pub struct End {
    pub dispatched_ms: u64,
    pub completed_ms: u64,
    pub response: Option<Content>,
    pub error: Option<String>,
    pub terminal_state: Option<crate::types::InvocationState>,
}

pub fn base(service: &Service) -> PathBuf {
    service.config.authority_root.join("model-interactions")
}
pub fn directory(service: &Service, input: (&str, &str)) -> PathBuf {
    base(service).join(digest(format!("{}/{}", input.0, input.1).as_bytes()))
}
pub fn artifact_id(identity: &CallIdentity, owner: &str) -> String {
    format!(
        "{PREFIX}.{}.{}",
        digest(format!("{owner}/{}", identity.reservation_id).as_bytes()),
        digest(format!("{owner}/{}", identity.invocation_id).as_bytes())
    )
}
pub fn parse_id(id: &str) -> Result<(&str, &str), String> {
    let parts = id.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts[0] != PREFIX
        || parts[1..].iter().any(|v| {
            v.len() != 64
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
    {
        return Err("not_found".into());
    }
    Ok((parts[1], parts[2]))
}
pub fn document_path(dir: &Path, id: &str, slot: &str) -> Result<PathBuf, String> {
    let (_, call) = parse_id(id)?;
    Ok(dir.join(format!("{call}.{slot}.json")))
}
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    if !fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_file()
    {
        return Err("invalid_diagnostic_document".into());
    }
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_DOCUMENT_BYTES {
        return Err("diagnostic_document_oversized".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
pub fn write_bounded<T: Serialize>(path: &Path, value: &T, bound: u64) -> Result<(), String> {
    let bytes = serde_json::to_string(value).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > bound.min(MAX_DOCUMENT_BYTES) {
        return Err("diagnostic_document_oversized".into());
    }
    atomic_write_private(path, &bytes)
}
pub fn lock(service: &Service) -> Result<fs::File, String> {
    private_directory(&base(service))?;
    rack_ai_infrastructure::resource_reservations::bounded_lock(
        &base(service).join("diagnostics.lock"),
    )
}
pub fn private_directory(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    if fs::symlink_metadata(dir).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink()) {
        return Err("invalid_diagnostic_directory".into());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| e.to_string())
}
pub fn lease(dir: &Path) -> Result<Lease, String> {
    read(&dir.join(LEASE_FILE))
}
pub fn save_lease(dir: &Path, lease: &Lease) -> Result<(), String> {
    write_bounded(&dir.join(LEASE_FILE), lease, METADATA_BYTES)
}
pub fn starts(dir: &Path, limit: usize) -> Result<Vec<Start>, String> {
    let mut calls = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name().to_string_lossy().ends_with(".start.json") {
            if calls.len() >= limit {
                return Err("diagnostic_record_limit".into());
            }
            calls.push(read(&entry.path())?);
        }
    }
    calls.sort_by_key(|c: &Start| c.identity.sequence);
    Ok(calls)
}

pub fn summaries(
    service: &Service,
    input: (&str, &crate::types::Invocation),
) -> Result<Option<Value>, String> {
    let (owner, parent) = input;
    let reservation = parent
        .request
        .work
        .as_ref()
        .map(|w| w.reservation_id.as_str())
        .ok_or("not_found")?;
    if !service.authority.read(|s| {
        Ok(crate::interaction_diagnostics::live(
            s,
            (owner, reservation),
        ))
    })? {
        return Ok(None);
    }
    let dir = directory(service, (owner, reservation));
    if store_base_invalid(service)? {
        return Err("diagnostic_storage_unavailable".into());
    }
    if !dir.exists() {
        return Ok(Some(
            serde_json::json!({"schema":crate::interaction_diagnostics::SCHEMA,
        "records":[],"omitted_interactions":0,"availability":"no_records"}),
        ));
    }
    let _lock = lock(service)?;
    let lease = lease(&dir)?;
    let calls = starts(&dir, lease.limits.max_interactions_per_reservation)?;
    let records = calls
        .into_iter()
        .filter(|c| c.identity.parent_invocation_id == parent.id)
        .map(|c| {
            serde_json::json!({"artifact_id":c.artifact_id,"invocation_id":c.identity.invocation_id,
            "sequence":c.identity.sequence,"request_truncated":c.request.truncated})
        })
        .collect::<Vec<_>>();
    if !service.authority.read(|s| {
        Ok(crate::interaction_diagnostics::live(
            s,
            (owner, reservation),
        ))
    })? {
        return Ok(None);
    }
    Ok(Some(
        serde_json::json!({"schema":crate::interaction_diagnostics::SCHEMA,"records":records,
        "omitted_interactions":lease.omitted_interactions,"omitted_scope":"reservation",
        "availability":"recorded","limits":lease.limits}),
    ))
}

pub fn retrieve(service: &Service, input: (&str, &str)) -> Result<Value, String> {
    let (owner, id) = input;
    let (reservation_key, _) = parse_id(id)?;
    let dir = base(service).join(reservation_key);
    let lease: Lease = lease(&dir).map_err(|_| "not_found")?;
    let visible = || {
        service.authority.read(|s| {
            Ok(lease.owner == owner
                && !lease.cleanup_pending
                && directory(service, (owner, &lease.reservation_id)) == dir
                && crate::interaction_diagnostics::live(s, (owner, &lease.reservation_id)))
        })
    };
    if !visible()? {
        return Err("not_found".into());
    }
    let _lock = lock(service)?;
    let start: Start = read(&document_path(&dir, id, "start")?).map_err(|_| "not_found")?;
    if artifact_id(&start.identity, owner) != id || !visible()? {
        return Err("not_found".into());
    }
    let end_path = document_path(&dir, id, "end")?;
    let end: Option<End> = if end_path.exists() {
        Some(read(&end_path)?)
    } else {
        None
    };
    let terminal = service.result(owner, &start.identity.invocation_id).ok();
    let terminal_state = terminal
        .as_ref()
        .map(|i| i.state)
        .or_else(|| end.as_ref().and_then(|e| e.terminal_state));
    let record = serde_json::json!({"schema":crate::interaction_diagnostics::SCHEMA,"artifact_id":id,
        "identity":start.identity,"request":start.request,
        "response":end.as_ref().and_then(|e| e.response.as_ref()),
        "error":end.as_ref().and_then(|e| e.error.as_ref()),
        "terminal_state":terminal_state,"diagnostic_complete":end.is_some(),
        "timing":{"dispatched_ms":end.as_ref().map(|e| e.dispatched_ms),"completed_ms":end.as_ref().map(|e| e.completed_ms),
            "duration_seconds":end.as_ref().map(|e| e.completed_ms.saturating_sub(e.dispatched_ms) as f64/1000.0),
            "prefill_seconds":null,"first_token_seconds":null,"backend_queue_seconds":null,
            "unavailable_reason":"backend_does_not_report_phase_metrics"}});
    if !visible()? {
        return Err("not_found".into());
    }
    Ok(record)
}
pub fn sweep(service: &Service) -> Result<(), String> {
    crate::interaction_cleanup::sweep(service)
}

fn store_base_invalid(service: &Service) -> Result<bool, String> {
    match fs::symlink_metadata(base(service)) {
        Ok(meta) => Ok(!meta.is_dir() || meta.file_type().is_symlink()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}
