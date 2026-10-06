//! Leftover bounded lease directories are durable cleanup obligations.
use crate::{
    interaction_diagnostics::{MAX_SWEEP, live},
    interaction_store as store,
    service::Service,
};
use std::fs;

pub fn sweep(service: &Service) -> Result<(), String> {
    let base = store::base(service);
    if !base.exists() {
        return Ok(());
    }
    let _lock = store::lock(service)?;
    let cursor_path = base.join("sweep-cursor");
    let cursor = fs::read_to_string(&cursor_path).unwrap_or_default();
    let mut selected = candidates(&base, &cursor)?;
    if selected.is_empty() && !cursor.is_empty() {
        selected = candidates(&base, "")?;
    }
    let mut failure = None;
    for name in &selected {
        let dir = base.join(name);
        let mut lease = match store::lease(&dir) {
            Ok(lease) => lease,
            Err(error) => {
                // An unreadable/orphan lease has no public visibility; never resurrect it.
                if let Err(delete_error) = fs::remove_dir_all(&dir) {
                    failure = Some(format!("{error}; {delete_error}"));
                }
                continue;
            }
        };
        let visible = service
            .authority
            .read(|s| Ok(live(s, (&lease.owner, &lease.reservation_id))))?;
        if visible && !lease.cleanup_pending {
            continue;
        }
        lease.cleanup_pending = true;
        // Visibility already fails against authority, even if this write also fails.
        if let Err(error) = store::save_lease(&dir, &lease) {
            failure = Some(error);
        }
        if let Err(error) = fs::remove_dir_all(&dir) {
            failure = Some(error.to_string());
        }
    }
    if let Some(last) = selected.last() {
        rack_ai_application::durable_file::atomic_write_private(&cursor_path, last)?;
    }
    failure.map_or(Ok(()), Err)
}

pub fn close(service: &Service, input: (&str, &str)) -> Result<(), String> {
    let dir = store::directory(service, input);
    if !dir.exists() {
        return Ok(());
    }
    let _lock = store::lock(service)?;
    if service.authority.read(|s| Ok(live(s, input)))? {
        return Ok(());
    }
    if let Ok(mut lease) = store::lease(&dir) {
        lease.cleanup_pending = true;
        if let Err(error) = store::save_lease(&dir, &lease) {
            eprintln!("interaction diagnostics cleanup marker pending: {error}");
        }
    }
    fs::remove_dir_all(dir).map_err(|e| e.to_string())
}

fn candidates(base: &std::path::Path, after: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(base).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.len() != 64
            || !name.bytes().all(|b| b.is_ascii_hexdigit())
            || name.as_str() <= after
        {
            continue;
        }
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        names.push(name);
        names.sort();
        names.truncate(MAX_SWEEP);
    }
    Ok(names)
}
