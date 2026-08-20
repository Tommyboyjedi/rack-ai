use std::fs;
use std::path::PathBuf;
use std::process::Command;

use rack_ai_application::CoderToolRunner;
use serde_json::Value;

pub struct HostCoderToolRunner {
    cwd: PathBuf,
}

impl HostCoderToolRunner {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

impl CoderToolRunner for HostCoderToolRunner {
    fn run(&self, name: &str, arguments: &Value) -> Result<String, String> {
        if name == "write" {
            return self.run_write(arguments);
        }
        if name == "read" {
            return self.run_read(arguments);
        }
        if name == "bash" {
            return self.run_bash(arguments);
        }
        Err(format!("Unsupported tool {name}"))
    }
}

impl HostCoderToolRunner {
    fn run_write(&self, arguments: &Value) -> Result<String, String> {
        let path = self.resolve_path(read_required_string(arguments, "file_path")?);
        let content = read_required_string(arguments, "content")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, content).map_err(|error| error.to_string())?;
        Ok(format!("Created {}", path.display()))
    }

    fn run_read(&self, arguments: &Value) -> Result<String, String> {
        let path = self.resolve_path(read_required_string(arguments, "file_path")?);
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let start_line = read_optional_u64(arguments, "start_line")
            .unwrap_or(1)
            .max(1) as usize;
        let limit = read_optional_u64(arguments, "limit").unwrap_or(400).max(1) as usize;
        let lines = text.lines().collect::<Vec<_>>();
        let selected = lines
            .iter()
            .enumerate()
            .skip(start_line.saturating_sub(1))
            .take(limit)
            .map(|(index, line)| format!("{}\t{}", index + 1, line))
            .collect::<Vec<_>>();
        Ok(selected.join("\n"))
    }

    fn run_bash(&self, arguments: &Value) -> Result<String, String> {
        let command = read_required_string(arguments, "command")?;
        let output = Command::new("sh")
            .arg("-lc")
            .arg(command)
            .current_dir(&self.cwd)
            .output()
            .map_err(|error| error.to_string())?;
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        text.push_str(String::from_utf8_lossy(&output.stderr).as_ref());
        Ok(text.trim().to_string())
    }

    fn resolve_path(&self, raw_path: &str) -> PathBuf {
        let path = PathBuf::from(raw_path);
        if path.is_absolute() {
            path
        } else {
            self.cwd.join(path)
        }
    }
}

fn read_required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(format!("missing string field: {key}"))
}

fn read_optional_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::HostCoderToolRunner;
    use rack_ai_application::CoderToolRunner;
    use serde_json::json;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_and_reads_relative_files() {
        let cwd = temp_root();
        let runner = HostCoderToolRunner::new(cwd);
        runner
            .run(
                "write",
                &json!({"file_path": "nested/out.txt", "content": "hello"}),
            )
            .unwrap();
        let read = runner
            .run("read", &json!({"file_path": "nested/out.txt"}))
            .unwrap();
        assert_eq!(read, "1\thello");
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-host-coder-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
