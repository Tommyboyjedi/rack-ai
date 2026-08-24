use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE: &str = include_str!("jcode_network_guard.c");

pub fn compile_at(root: &Path) -> Result<PathBuf, String> {
    let source_path = root.join("jcode_network_guard.c");
    let shared_object = root.join("jcode_network_guard.so");
    fs::write(&source_path, SOURCE).map_err(|error| error.to_string())?;
    let output = Command::new("cc")
        .arg("-shared")
        .arg("-fPIC")
        .arg("-O2")
        .arg("-Wall")
        .arg("-Werror")
        .arg("-o")
        .arg(&shared_object)
        .arg(&source_path)
        .arg("-ldl")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "failed to compile JCode network guard: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(shared_object)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::compile_at;

    #[test]
    fn compiles_shared_object() {
        let root = temp_root();
        let shared_object = compile_at(&root).unwrap();
        assert!(shared_object.exists());
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-netguard-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
