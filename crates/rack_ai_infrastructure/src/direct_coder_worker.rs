use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use serde_json::json;

pub struct DirectCoderWorker {
    endpoint: String,
    system_prompt: String,
}

impl DirectCoderWorker {
    pub fn local_default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8018/v1/chat/completions".to_string(),
            system_prompt: ("You are a coding worker. Use tools when needed. ".to_string()
                + "When the task is complete, stop calling tools and reply exactly as requested."),
        }
    }

    pub fn execute(&self, task: &str, cwd: &Path, max_turns: usize) -> Result<String, String> {
        let mut messages = vec![
            json!({"role": "system", "content": self.system_prompt}),
            json!({"role": "user", "content": self.build_prompt(task)}),
        ];
        for _ in 0..max_turns {
            let response = self.call_api(&messages)?;
            let choice = response
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .ok_or("response did not contain a choice".to_string())?;
            let message = choice
                .get("message")
                .cloned()
                .ok_or("response choice did not contain a message".to_string())?;
            let finish_reason = choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            messages.push(message.clone());
            if finish_reason == "stop" {
                return Ok(message
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string());
            }
            if finish_reason != "tool_calls" {
                return Err(format!("unexpected finish_reason: {finish_reason}"));
            }
            let tool_calls = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .ok_or("tool_calls finish_reason without tool_calls".to_string())?;
            for tool_call in tool_calls {
                let tool_call_id = tool_call
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("tool call missing id".to_string())?;
                let function = tool_call
                    .get("function")
                    .and_then(Value::as_object)
                    .ok_or("tool call missing function".to_string())?;
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("tool call function missing name".to_string())?;
                let arguments = function
                    .get("arguments")
                    .and_then(Value::as_str)
                    .ok_or("tool call function missing arguments".to_string())?;
                let parsed_arguments =
                    serde_json::from_str::<Value>(arguments).map_err(|error| error.to_string())?;
                let result = self.run_tool(name, &parsed_arguments, cwd)?;
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": result,
                }));
            }
        }
        Err("max turns exceeded".to_string())
    }

    fn build_prompt(&self, task: &str) -> String {
        format!(
            "Using your tools, complete the following task exactly as requested.\n\nRules:\n- Use actual tool calls.\n- Do not describe tool calls in plain text.\n- After the requested file action is confirmed, reply with exactly COMPLETE and stop.\n- Do not write the word COMPLETE into any project file unless explicitly asked.\n\nTask:\n{task}"
        )
    }

    fn call_api(&self, messages: &[Value]) -> Result<Value, String> {
        let payload = json!({
            "model": "local-coder",
            "messages": messages,
            "tools": self.tool_definitions(),
            "tool_choice": "auto",
            "stream": false,
            "temperature": 0,
        });
        let mut response = ureq::post(&self.endpoint)
            .send_json(&payload)
            .map_err(|error| error.to_string())?;
        response
            .body_mut()
            .read_json::<Value>()
            .map_err(|error| error.to_string())
    }

    fn tool_definitions(&self) -> Value {
        json!([
            {
                "type": "function",
                "function": {
                    "name": "write",
                    "description": "Write a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"},
                            "content": {"type": "string"},
                            "intent": {"type": "string"}
                        },
                        "required": ["file_path", "content"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "read",
                    "description": "Read a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"},
                            "start_line": {"type": "integer"},
                            "limit": {"type": "integer"},
                            "intent": {"type": "string"}
                        },
                        "required": ["file_path"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "bash",
                    "description": "Run a shell command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"},
                            "description": {"type": "string"}
                        },
                        "required": ["command"]
                    }
                }
            }
        ])
    }

    fn run_tool(&self, name: &str, arguments: &Value, cwd: &Path) -> Result<String, String> {
        if name == "write" {
            return self.run_write(arguments, cwd);
        }
        if name == "read" {
            return self.run_read(arguments, cwd);
        }
        if name == "bash" {
            return self.run_bash(arguments, cwd);
        }
        Err(format!("Unsupported tool {name}"))
    }

    fn run_write(&self, arguments: &Value, cwd: &Path) -> Result<String, String> {
        let path = self.resolve_path(read_required_string(arguments, "file_path")?, cwd);
        let content = read_required_string(arguments, "content")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, content).map_err(|error| error.to_string())?;
        Ok(format!("Created {}", path.display()))
    }

    fn run_read(&self, arguments: &Value, cwd: &Path) -> Result<String, String> {
        let path = self.resolve_path(read_required_string(arguments, "file_path")?, cwd);
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

    fn run_bash(&self, arguments: &Value, cwd: &Path) -> Result<String, String> {
        let command = read_required_string(arguments, "command")?;
        let output = Command::new("sh")
            .arg("-lc")
            .arg(command)
            .current_dir(cwd)
            .output()
            .map_err(|error| error.to_string())?;
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        text.push_str(String::from_utf8_lossy(&output.stderr).as_ref());
        Ok(text.trim().to_string())
    }

    fn resolve_path(&self, raw_path: &str, cwd: &Path) -> PathBuf {
        let path = PathBuf::from(raw_path);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
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
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::DirectCoderWorker;

    #[test]
    fn builds_completion_prompt() {
        let worker = DirectCoderWorker::local_default();
        let prompt = worker.build_prompt("Write file");
        assert!(prompt.contains("Use actual tool calls."));
        assert!(prompt.contains("Task:\nWrite file"));
    }

    #[test]
    fn writes_and_reads_files_relative_to_cwd() {
        let worker = DirectCoderWorker::local_default();
        let cwd = temp_root();
        let write_result = worker
            .run_tool(
                "write",
                &json!({"file_path": "nested/out.txt", "content": "hello"}),
                &cwd,
            )
            .unwrap();
        let read_result = worker
            .run_tool("read", &json!({"file_path": "nested/out.txt"}), &cwd)
            .unwrap();
        assert!(write_result.contains("nested/out.txt"));
        assert_eq!(read_result, "1\thello");
    }

    #[test]
    fn runs_bash_in_requested_working_directory() {
        let worker = DirectCoderWorker::local_default();
        let cwd = temp_root();
        fs::write(cwd.join("marker.txt"), "marker\n").unwrap();
        let result = worker
            .run_tool("bash", &json!({"command": "cat marker.txt"}), &cwd)
            .unwrap();
        assert_eq!(result, "marker");
    }

    fn temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-direct-coder-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
