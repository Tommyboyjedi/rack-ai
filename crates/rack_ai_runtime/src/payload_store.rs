//! Essential active request/result bodies live outside the compact authority document.
use crate::types::*;
use rack_ai_application::durable_file::atomic_write;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy)]
pub enum PayloadSlot {
    Request,
    Result,
    LateResult,
}

impl PayloadSlot {
    fn name(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Result => "result",
            Self::LateResult => "late_result",
        }
    }
}

pub fn store_request(root: &Path, invocation: &mut Invocation) -> Result<(), String> {
    let request = invocation.request.clone();
    let reference = store(
        root,
        &invocation.owner,
        &invocation.id,
        PayloadSlot::Request,
        &request,
    )?;
    invocation
        .request_digest
        .get_or_insert(reference.sha256.clone());
    invocation.request_bytes.get_or_insert(reference.bytes);
    if let Some(work) = request.work.as_ref() {
        let bytes = json_bytes(work)?;
        invocation.work_digest.get_or_insert_with(|| digest(&bytes));
        invocation.work_bytes.get_or_insert(bytes.len() as u64);
    }
    invocation.request_ref = Some(reference);
    compact_request(&mut invocation.request);
    Ok(())
}

pub fn store_result(root: &Path, invocation: &mut Invocation, value: Value) -> Result<(), String> {
    let reference = store(
        root,
        &invocation.owner,
        &invocation.id,
        PayloadSlot::Result,
        &value,
    )?;
    invocation.result_digest = Some(reference.sha256.clone());
    invocation.result_ref = Some(reference);
    invocation.result = None;
    Ok(())
}

pub fn store_late_result(
    root: &Path,
    invocation: &mut Invocation,
    value: Value,
) -> Result<(), String> {
    let reference = store(
        root,
        &invocation.owner,
        &invocation.id,
        PayloadSlot::LateResult,
        &value,
    )?;
    invocation.late_result_digest = Some(reference.sha256.clone());
    invocation.late_result_ref = Some(reference);
    invocation.late_result = None;
    Ok(())
}

pub fn full_request(root: &Path, invocation: &Invocation) -> Result<Inference, String> {
    if !compacted(&invocation.request) {
        return Ok(invocation.request.clone());
    }
    match &invocation.request_ref {
        Some(reference) => read(root, reference),
        None => Ok(invocation.request.clone()),
    }
}

pub fn hydrate_invocation(root: &Path, mut invocation: Invocation) -> Result<Invocation, String> {
    if invocation.request_ref.is_some() {
        invocation.request = full_request(root, &invocation)?;
    }
    if let Some(reference) = &invocation.result_ref {
        invocation.result = Some(read(root, reference)?);
    }
    if let Some(reference) = &invocation.late_result_ref {
        invocation.late_result = Some(read(root, reference)?);
    }
    Ok(invocation)
}

pub fn remove_reference(root: &Path, reference: &StoredPayloadRef) -> Result<(), String> {
    let path = reference_path(root, reference)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn remove_invocation_payloads(root: &Path, invocation: &Invocation) -> Result<(), String> {
    for reference in [
        invocation.request_ref.as_ref(),
        invocation.result_ref.as_ref(),
        invocation.late_result_ref.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        remove_reference(root, reference)?;
    }
    Ok(())
}

pub fn remove_uncommitted_result_payloads(
    root: &Path,
    invocation: &Invocation,
    previous_result_ref: Option<&StoredPayloadRef>,
    previous_late_result_ref: Option<&StoredPayloadRef>,
) -> Result<(), String> {
    if let Some(reference) = invocation.result_ref.as_ref() {
        if previous_result_ref != Some(reference) {
            remove_reference(root, reference)?;
        }
    }
    if let Some(reference) = invocation.late_result_ref.as_ref() {
        if previous_late_result_ref != Some(reference) {
            remove_reference(root, reference)?;
        }
    }
    Ok(())
}

pub fn active_payload_commitment(invocation: &Invocation) -> u64 {
    request_bytes(invocation).saturating_add(response_storage_commitment(invocation))
}

pub fn response_storage_bound(response_bytes: u64) -> u64 {
    response_bytes.saturating_mul(2).saturating_add(4096)
}

fn response_storage_commitment(invocation: &Invocation) -> u64 {
    let stored = invocation
        .result_ref
        .as_ref()
        .map(|reference| reference.bytes)
        .unwrap_or(0)
        .saturating_add(
            invocation
                .late_result_ref
                .as_ref()
                .map(|reference| reference.bytes)
                .unwrap_or(0),
        );
    response_storage_bound(invocation.response_bytes).max(stored)
}

fn request_bytes(invocation: &Invocation) -> u64 {
    invocation
        .request_ref
        .as_ref()
        .map(|reference| reference.bytes)
        .or(invocation.request_bytes)
        .unwrap_or_else(|| {
            json_bytes(&invocation.request)
                .map(|bytes| bytes.len() as u64)
                .unwrap_or(0)
        })
}

fn store<T: Serialize>(
    root: &Path,
    owner: &str,
    invocation: &str,
    slot: PayloadSlot,
    value: &T,
) -> Result<StoredPayloadRef, String> {
    let bytes = json_bytes(value)?;
    let relative = relative_path(owner, invocation, slot)?;
    let path = root.join(&relative);
    let contents = String::from_utf8(bytes.clone()).map_err(|_| "payload_not_utf8".to_string())?;
    atomic_write(&path, &contents)?;
    Ok(StoredPayloadRef {
        sha256: digest(&bytes),
        bytes: bytes.len() as u64,
        path: relative.to_string_lossy().into_owned(),
    })
}

fn read<T: DeserializeOwned>(root: &Path, reference: &StoredPayloadRef) -> Result<T, String> {
    let path = reference_path(root, reference)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "payload_unavailable".to_string())?
        .take(reference.bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "payload_unavailable".to_string())?;
    if bytes.len() as u64 != reference.bytes || digest(&bytes) != reference.sha256 {
        return Err("payload_integrity_failure".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "payload_decode_failure".into())
}

fn relative_path(owner: &str, invocation: &str, slot: PayloadSlot) -> Result<PathBuf, String> {
    if !valid_id(invocation) {
        return Err("invalid_payload_identity".into());
    }
    Ok(PathBuf::from("payloads")
        .join("owners")
        .join(digest(owner.as_bytes()))
        .join("invocations")
        .join(digest(invocation.as_bytes()))
        .join(format!("{}.json", slot.name())))
}

fn reference_path(root: &Path, reference: &StoredPayloadRef) -> Result<PathBuf, String> {
    let path = Path::new(&reference.path);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("invalid_payload_reference".into());
    }
    Ok(root.join(path))
}

fn compacted(request: &Inference) -> bool {
    request.prompt.is_empty() && request.payload.is_none()
}

fn compact_request(request: &mut Inference) {
    request.prompt.clear();
    request.payload = None;
    if let Some(work) = request.work.as_mut() {
        match &mut work.payload {
            crate::work_payload::Payload::Inference { prompt, .. } => prompt.clear(),
            crate::work_payload::Payload::Workspace { workspace } => workspace.objective.clear(),
        }
    }
}
