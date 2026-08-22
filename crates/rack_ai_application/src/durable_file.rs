use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

pub fn atomic_write(path: &Path, contents: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| {
        format!("cannot write {} without a parent directory", path.display())
    })?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let tmp = temporary_path(path);
    {
        let mut file = File::create(&tmp).map_err(|error| error.to_string())?;
        file.write_all(contents.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        error.to_string()
    })?;
    sync_directory(parent)
}

pub fn append_line(path: &Path, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    writeln!(file, "{line}").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

pub struct CampaignLock {
    _file: File,
}

impl CampaignLock {
    pub fn acquire(campaign_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(campaign_dir).map_err(|error| error.to_string())?;
        let path = campaign_dir.join("control.lock");
        Self::acquire_path(&path)
    }

    pub fn acquire_path(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        file.lock().map_err(|error| error.to_string())?;
        Ok(Self { _file: file })
    }
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(target_family = "unix")]
    {
        File::open(path)
            .map_err(|error| error.to_string())?
            .sync_all()
            .map_err(|error| error.to_string())
    }
    #[cfg(not(target_family = "unix"))]
    {
        let _ = path;
        Ok()
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "state".to_string());
    path.with_file_name(format!(
        ".{name}.{}.tmp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

#[cfg(test)]
mod tests {
    use super::atomic_write;
    use super::CampaignLock;
    use std::fs;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn interrupted_temp_file_does_not_replace_durable_state() {
        let root = temp_root();
        let path = root.join("state.json");
        atomic_write(&path, "{\"ok\":true}\n").unwrap();
        fs::write(root.join(".state.json.partial.tmp"), "{\"torn\":true}\n").unwrap();
        let loaded = fs::read_to_string(&path).unwrap();
        assert!(loaded.contains("\"ok\":true"));
        assert!(!loaded.contains("torn"));
    }

    #[test]
    fn lock_serializes_operator_and_runner_writes() {
        let root = temp_root();
        let path = root.join("state.json");
        atomic_write(&path, "0\n").unwrap();
        let root = Arc::new(root);
        let mut joins = Vec::new();
        for _ in 0..8 {
            let root = Arc::clone(&root);
            joins.push(thread::spawn(move || {
                let _lock = CampaignLock::acquire(&root).unwrap();
                let current: u32 = fs::read_to_string(root.join("state.json"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                atomic_write(&root.join("state.json"), &format!("{}\n", current + 1)).unwrap();
            }));
        }
        for join in joins {
            join.join().unwrap();
        }
        let final_value: u32 = fs::read_to_string(path).unwrap().trim().parse().unwrap();
        assert_eq!(final_value, 8);
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-durable-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
