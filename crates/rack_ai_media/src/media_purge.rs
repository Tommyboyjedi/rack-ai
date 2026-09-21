//! End-of-session ComfyUI artifact purge.
//!
//! This removes the configured ComfyUI input/output tree contents after the
//! owned backend is stopped. It overwrites regular files before unlinking them,
//! but storage hardware, snapshots, journals and COW filesystems can still retain
//! historical blocks outside RackAI's control.
use crate::config::Config;
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::Path,
};

const CHUNK: usize = 1024 * 1024;

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct PurgeReport {
    pub roots: usize,
    pub files: usize,
    pub directories: usize,
    pub symlinks: usize,
    pub special_files: usize,
    pub overwritten_bytes: u64,
}

pub fn purge_configured(config: &Config) -> Result<PurgeReport, String> {
    let input_root = config.input_root();
    let mut report = PurgeReport::default();
    purge_root(&input_root, OsStr::new("input"), &mut report)?;
    purge_root(&config.output_root, OsStr::new("output"), &mut report)?;
    Ok(report)
}

fn purge_root(root: &Path, expected_name: &OsStr, report: &mut PurgeReport) -> Result<(), String> {
    validate_root(root, expected_name)?;
    let meta = match fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("comfyui_purge_root_unreadable:{error}")),
    };
    if meta.file_type().is_symlink() {
        return Err("comfyui_purge_root_symlink".into());
    }
    if !meta.is_dir() {
        return Err("comfyui_purge_root_not_directory".into());
    }
    for entry in fs::read_dir(root).map_err(|error| format!("comfyui_purge_read_dir:{error}"))? {
        let entry = entry.map_err(|error| format!("comfyui_purge_read_dir:{error}"))?;
        purge_entry(&entry.path(), report)?;
    }
    sync_dir(root)?;
    if fs::read_dir(root)
        .map_err(|error| format!("comfyui_purge_verify_root:{error}"))?
        .next()
        .transpose()
        .map_err(|error| format!("comfyui_purge_verify_root:{error}"))?
        .is_some()
    {
        return Err("comfyui_purge_root_not_empty".into());
    }
    report.roots += 1;
    Ok(())
}

fn validate_root(root: &Path, expected_name: &OsStr) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("comfyui_purge_root_not_absolute".into());
    }
    if root.file_name() != Some(expected_name) || root.parent().and_then(Path::parent).is_none() {
        return Err("comfyui_purge_root_not_bounded".into());
    }
    Ok(())
}

fn purge_entry(path: &Path, report: &mut PurgeReport) -> Result<(), String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("comfyui_purge_entry_unreadable:{error}")),
    };
    let ty = meta.file_type();
    if ty.is_symlink() {
        fs::remove_file(path).map_err(|error| format!("comfyui_purge_remove_symlink:{error}"))?;
        report.symlinks += 1;
        return Ok(());
    }
    if meta.is_dir() {
        for entry in
            fs::read_dir(path).map_err(|error| format!("comfyui_purge_read_dir:{error}"))?
        {
            let entry = entry.map_err(|error| format!("comfyui_purge_read_dir:{error}"))?;
            purge_entry(&entry.path(), report)?;
        }
        sync_dir(path)?;
        fs::remove_dir(path).map_err(|error| format!("comfyui_purge_remove_dir:{error}"))?;
        sync_parent(path)?;
        report.directories += 1;
        return Ok(());
    }
    if meta.is_file() {
        overwrite_regular(path, &meta)?;
        fs::remove_file(path).map_err(|error| format!("comfyui_purge_remove_file:{error}"))?;
        sync_parent(path)?;
        report.files += 1;
        report.overwritten_bytes = report.overwritten_bytes.saturating_add(meta.len());
        return Ok(());
    }
    if ty.is_fifo() || ty.is_socket() || ty.is_char_device() || ty.is_block_device() {
        fs::remove_file(path).map_err(|error| format!("comfyui_purge_remove_special:{error}"))?;
        sync_parent(path)?;
        report.special_files += 1;
        return Ok(());
    }
    Err("comfyui_purge_unknown_file_type".into())
}

fn overwrite_regular(path: &Path, meta: &fs::Metadata) -> Result<(), String> {
    if meta.nlink() > 1 {
        return Err("comfyui_purge_hardlink_unproven".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| format!("comfyui_purge_open_file:{error}"))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("comfyui_purge_seek_file:{error}"))?;
    let zeros = vec![0_u8; CHUNK];
    let mut remaining = meta.len();
    while remaining > 0 {
        let count = remaining.min(CHUNK as u64) as usize;
        file.write_all(&zeros[..count])
            .map_err(|error| format!("comfyui_purge_overwrite_file:{error}"))?;
        remaining -= count as u64;
    }
    file.sync_all()
        .map_err(|error| format!("comfyui_purge_sync_file:{error}"))?;
    file.set_len(0)
        .map_err(|error| format!("comfyui_purge_truncate_file:{error}"))?;
    file.sync_all()
        .map_err(|error| format!("comfyui_purge_sync_truncated_file:{error}"))
}

fn sync_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

fn sync_dir(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("comfyui_purge_sync_dir:{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Principal, Profile};
    use std::{os::unix::fs as unix_fs, path::PathBuf};

    fn root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "rack-media-purge-{name}-{}",
            crate::types::identity()
        ));
        fs::create_dir_all(path.join("input/nested")).unwrap();
        fs::create_dir_all(path.join("output/rack/job")).unwrap();
        path
    }

    fn config(root: &Path, input_root: Option<PathBuf>) -> Config {
        Config {
            listen: "127.0.0.1:18191".parse().unwrap(),
            native_listen: "127.0.0.1:18192".parse().unwrap(),
            public_origin: "http://127.0.0.1:18191/".into(),
            native_origin: "http://127.0.0.1:18192/".into(),
            state_root: root.join("state"),
            browser_auth_file: None,
            resource_root: root.join("resources"),
            input_root,
            output_root: root.join("output"),
            authority_file: root.join("authority.json"),
            control_secret_file: root.join("control"),
            unit: "rack-ai-comfyui-test.service".into(),
            runtime: crate::process_identity::RuntimePaths {
                python: root.join("python"),
                script: root.join("main.py"),
                directory: root.join("ComfyUI"),
            },
            backend: "http://127.0.0.1:8190/".into(),
            media_uuid: "GPU-test".into(),
            protected: vec![],
            profile: Profile {
                id: "local-image".into(),
                version: 1,
                checkpoint: root.join("checkpoint.safetensors"),
                checkpoint_sha256: "0".repeat(64),
                runtime_revision: "test".into(),
                available: true,
            },
            start_timeout: 1,
            drain_timeout: 1,
            stop_timeout: 1,
            idle_seconds: 1,
            reservation_idle_seconds: 1,
            session_seconds: 1,
            min_memory_mb: 1,
            min_disk_mb: 1,
            principals: vec![Principal {
                id: "operator".into(),
                token_sha256: "1".repeat(64),
                operator: true,
            }],
        }
    }

    #[test]
    fn purges_input_and_output_contents_without_removing_roots() {
        let root = root("contents");
        fs::write(root.join("input/upload.png"), b"secret input").unwrap();
        fs::write(root.join("input/nested/mask.png"), b"mask").unwrap();
        fs::write(root.join("output/rack/job/image.png"), b"secret output").unwrap();
        fs::write(root.join("outside.txt"), b"keep").unwrap();
        let outside_target = root.join("outside-target.txt");
        fs::write(&outside_target, b"do not touch").unwrap();
        unix_fs::symlink(&outside_target, root.join("input/link")).unwrap();

        let report = purge_configured(&config(&root, None)).unwrap();

        assert_eq!(report.roots, 2);
        assert_eq!(report.files, 3);
        assert_eq!(report.symlinks, 1);
        assert!(root.join("input").is_dir());
        assert!(root.join("output").is_dir());
        assert!(fs::read_dir(root.join("input")).unwrap().next().is_none());
        assert!(fs::read_dir(root.join("output")).unwrap().next().is_none());
        assert_eq!(fs::read(root.join("outside.txt")).unwrap(), b"keep");
        assert_eq!(fs::read(outside_target).unwrap(), b"do not touch");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_input_root_overrides_default_sibling() {
        let root = root("explicit");
        let explicit = root.join("input");
        fs::write(explicit.join("upload.png"), b"secret").unwrap();
        let report = purge_configured(&config(&root, Some(explicit))).unwrap();
        assert_eq!(report.roots, 2);
        assert!(fs::read_dir(root.join("input")).unwrap().next().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_symlink_roots_and_preserves_target() {
        let root = root("symlink-root");
        let target = root.join("real-input");
        fs::create_dir_all(&target).unwrap();
        fs::remove_dir_all(root.join("input")).unwrap();
        unix_fs::symlink(&target, root.join("input")).unwrap();
        fs::write(target.join("secret.png"), b"secret").unwrap();

        let error = purge_configured(&config(&root, None)).unwrap_err();

        assert_eq!(error, "comfyui_purge_root_symlink");
        assert_eq!(fs::read(target.join("secret.png")).unwrap(), b"secret");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn config_policy_requires_sibling_input_output_roots() {
        let root = root("policy-sibling");
        let cfg = config(&root, Some(root.join("uploads")));

        let error = cfg.validate().unwrap_err();

        assert_eq!(
            error,
            "ComfyUI cleanup roots must be sibling input/output directories"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn config_policy_rejects_broad_cleanup_roots() {
        let root = root("policy-broad");
        let mut cfg = config(&root, Some(PathBuf::from("/input")));
        cfg.output_root = PathBuf::from("/output");

        let error = cfg.validate().unwrap_err();

        assert_eq!(error, "ComfyUI cleanup roots are too broad");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hardlinked_files_fail_closed() {
        let root = root("hardlink");
        let first = root.join("input/upload.png");
        let second = root.join("input/hardlink.png");
        fs::write(&first, b"secret").unwrap();
        fs::hard_link(&first, &second).unwrap();

        let error = purge_configured(&config(&root, None)).unwrap_err();

        assert_eq!(error, "comfyui_purge_hardlink_unproven");
        assert!(first.exists());
        assert!(second.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
