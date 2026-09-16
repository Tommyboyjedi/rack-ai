//! Atomic updates to the administrator-selected registry already consumed by the worker.
use crate::{
    config::{Backend, Config},
    types::{digest, valid_id},
};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
const MAX_REGISTRY_BYTES: u64 = 65536;
const MAX_VOICES: usize = 128;
const MAX_RETAINED_BYTES: u64 = 768 * 1024 * 1024;
const MAX_RETAINED_FILES: usize = 1024;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    root: PathBuf,
    voices: BTreeMap<String, Entry>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    file: PathBuf,
    sha256: String,
}
pub struct Upload {
    pub voice_id: String,
    pub bytes: Vec<u8>,
}
#[derive(Serialize)]
pub struct Registered {
    voice_id: String,
    registered: bool,
    sha256: String,
}
pub fn safe_id(id: &str) -> bool {
    valid_id(id) && !id.starts_with('.') && !id.contains("..")
}
pub fn configured_path(config: &Config) -> Result<PathBuf, String> {
    let mut selected = None;
    for p in config
        .profiles
        .iter()
        .filter(|p| p.backend == Backend::Chatterbox)
    {
        let mut paths = p.args.iter().enumerate().filter_map(|(index, arg)| {
            if arg == "--voices" {
                Some(p.args.get(index + 1).map(String::as_str).unwrap_or(""))
            } else {
                arg.strip_prefix("--voices=")
            }
        });
        let path = PathBuf::from(paths.next().ok_or("voice_registry_unconfigured")?);
        if !path.is_absolute()
            || paths.next().is_some()
            || selected.as_ref().is_some_and(|s| s != &path)
        {
            return Err("voice_registry_unconfigured".into());
        }
        selected = Some(path);
    }
    selected.ok_or("voice_registry_unconfigured".into())
}
pub fn register(path: &Path, upload: Upload) -> Result<Registered, String> {
    if !safe_id(&upload.voice_id) {
        return Err("invalid_voice_id".into());
    }
    crate::reference_audio::validate(&upload.bytes)?;
    canonical(path)?;
    let lock_path = path.with_extension("register.lock");
    if fs::symlink_metadata(&lock_path).is_ok() {
        canonical(&lock_path)?;
    }
    let _lock = rack_ai_infrastructure::resource_reservations::bounded_lock(&lock_path)
        .map_err(|_| "voice_registry_unavailable")?;
    let mut registry = load(path)?;
    if !registry.voices.contains_key(&upload.voice_id) && registry.voices.len() >= MAX_VOICES {
        return Err("voice_registry_full".into());
    }
    let hash = digest(&upload.bytes);
    let name = format!("registered-{hash}.wav");
    let destination = registry.root.join(&name);
    store_audio(&destination, &upload.bytes)?;
    registry.voices.insert(
        upload.voice_id.clone(),
        Entry {
            file: name.into(),
            sha256: hash.clone(),
        },
    );
    let contents = serde_json::to_string(&registry).map_err(|_| "voice_registry_invalid")?;
    if contents.len() as u64 > MAX_REGISTRY_BYTES {
        return Err("voice_registry_full".into());
    }
    rack_ai_application::durable_file::atomic_write_private(path, &contents)
        .map_err(|_| "voice_registry_write_failed")?;
    Ok(Registered {
        voice_id: upload.voice_id,
        registered: true,
        sha256: hash,
    })
}
fn canonical(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || fs::canonicalize(path).map_err(|_| "voice_storage_unavailable")? != path
    {
        return Err("unsafe_voice_storage".into());
    }
    Ok(())
}
fn load(path: &Path) -> Result<Registry, String> {
    canonical(path)?;
    let metadata = fs::metadata(path).map_err(|_| "voice_registry_unavailable")?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err("unsafe_voice_registry".into());
    }
    let mut raw = Vec::new();
    fs::File::open(path)
        .map_err(|_| "voice_registry_unavailable")?
        .take(MAX_REGISTRY_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "voice_registry_unavailable")?;
    if raw.len() as u64 > MAX_REGISTRY_BYTES {
        return Err("voice_registry_full".into());
    }
    let registry: Registry = serde_json::from_slice(&raw).map_err(|_| "voice_registry_invalid")?;
    canonical(&registry.root)?;
    if !registry.root.is_dir() || registry.voices.len() > MAX_VOICES {
        return Err("voice_registry_invalid".into());
    }
    for (id, entry) in &registry.voices {
        if !safe_id(id)
            || entry.file.as_os_str().is_empty()
            || entry
                .file
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || entry.sha256.len() != 64
            || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("voice_registry_invalid".into());
        }
        canonical(&registry.root.join(&entry.file))?;
    }
    Ok(registry)
}
fn store_audio(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        capacity(
            path.parent().ok_or("voice_storage_unavailable")?,
            bytes.len() as u64,
        )?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "voice_storage_write_failed")?;
            fs::File::open(path.parent().ok_or("voice_storage_unavailable")?)
                .and_then(|dir| dir.sync_all())
                .map_err(|_| "voice_storage_write_failed")?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            canonical(path)?;
            let metadata = fs::metadata(path).map_err(|_| "voice_storage_unavailable")?;
            if !metadata.is_file()
                || metadata.len() != bytes.len() as u64
                || digest(&fs::read(path).map_err(|_| "voice_storage_unavailable")?)
                    != digest(bytes)
            {
                return Err("voice_storage_integrity_failure".into());
            }
        }
        Err(_) => return Err("voice_storage_write_failed".into()),
    }
    Ok(())
}

// Includes superseded blobs so replacement cannot evade the unauthenticated upload bound.
fn capacity(root: &Path, incoming: u64) -> Result<(), String> {
    let mut bytes = incoming;
    let mut files = 1;
    for entry in fs::read_dir(root).map_err(|_| "voice_storage_unavailable")? {
        let entry = entry.map_err(|_| "voice_storage_unavailable")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("registered-") || !name.ends_with(".wav") {
            continue;
        }
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|_| "voice_storage_unavailable")?;
        if !metadata.is_file() {
            return Err("unsafe_voice_storage".into());
        }
        files += 1;
        bytes = bytes.saturating_add(metadata.len());
        if bytes > MAX_RETAINED_BYTES || files > MAX_RETAINED_FILES {
            return Err("voice_registry_full".into());
        }
    }
    Ok(())
}
