use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use rack_ai_application::CoderRunRequest;
use rack_ai_application::CoderToolRunner;
use serde_json::Value;
use serde_json::json;

use crate::HostCoderToolRunner;

pub struct DirectCoderWorker {
    endpoint: String,
    model_id: String,
    system_prompt: String,
}

impl DirectCoderWorker {
    pub fn local_default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8018/v1/chat/completions".to_string(),
            model_id: "local-coder".to_string(),
            system_prompt: Self::default_system_prompt(),
        }
    }

    pub fn new(endpoint: String, model_id: String, system_prompt: String) -> Self {
        Self {
            endpoint: normalize_endpoint(endpoint),
            model_id,
            system_prompt,
        }
    }

    pub fn default_system_prompt() -> String {
        "You are a coding worker. Use tools when needed. When the task is complete, stop calling tools and reply exactly as requested.".to_string()
    }

    pub fn execute(&self, task: &str, cwd: &Path, max_turns: usize) -> Result<String, String> {
        let runner = HostCoderToolRunner::new(cwd.to_path_buf());
        self.execute_with_runner(&CoderRunRequest::new(task.to_string(), max_turns)?, &runner)
    }

    pub fn execute_with_runner(
        &self,
        request: &CoderRunRequest,
        runner: &dyn CoderToolRunner,
    ) -> Result<String, String> {
        let mut messages = vec![
            json!({"role": "system", "content": self.system_prompt}),
            json!({"role": "user", "content": self.build_prompt(request.task())}),
        ];
        let deadline = request
            .timeout_seconds()
            .map(|seconds| Instant::now() + Duration::from_secs(u64::from(seconds.max(1))));
        for _ in 0..request.max_turns() {
            if let Some(deadline) = deadline {
                if Instant::now() >= deadline {
                    return Err("coder wall-clock timeout exceeded".to_string());
                }
            }
            let request_timeout = deadline
                .map(|limit| limit.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_secs(330));
            if request_timeout.is_zero() {
                return Err("coder wall-clock timeout exceeded".to_string());
            }
            let response = self.call_api(&messages, request_timeout)?;
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
                let result = runner.run(name, &parsed_arguments)?;
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
                "Using your tools, complete the following task exactly as requested.\n\n\
    Worker identity:\n\
    - Your assigned model id is `{}`.\n\
    - This model id is authoritative.\n\
    - Do not infer, guess, or probe your model identity using shell commands, environment variables, files, or tools.\n\n\
    Rules:\n\
    - Use actual tool calls.\n\
    - Do not describe tool calls in plain text.\n\
    - After the requested file action is confirmed, reply with exactly COMPLETE and stop.\n\
    - Do not write the word COMPLETE into any project file unless explicitly asked.\n\n\
    Task:\n{}",
            self.model_id,
            task
        )
    }

    fn call_api(&self, messages: &[Value], request_timeout: Duration) -> Result<Value, String> {
        let payload = json!({
        "model": self.model_id,
        "messages": messages,
        "tools": self.tool_definitions(),
        "tool_choice": "auto",
        "stream": false,
        "temperature": 0,
        "max_tokens": 1024,
    });

        let turn_timeout = request_timeout.min(Duration::from_secs(60));

        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_send_request(Some(turn_timeout))
            .timeout_recv_response(Some(turn_timeout))
            .timeout_global(Some(turn_timeout))
            .build();

        let agent = config.new_agent();

        let mut response = agent
            .post(&self.endpoint)
            .send_json(&payload)
            .map_err(|error| format!("model request failed or timed out: {error}"))?;

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

    #[cfg(test)]
    fn run_tool(&self, name: &str, arguments: &Value, cwd: &Path) -> Result<String, String> {
        HostCoderToolRunner::new(cwd.to_path_buf()).run(name, arguments)
    }
}

fn normalize_endpoint(endpoint: String) -> String {
    if endpoint.ends_with("/chat/completions") {
        endpoint
    } else {
        format!("{}/chat/completions", endpoint.trim_end_matches('/'))
    }
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
        assert!(prompt.contains("Your assigned model id is `local-coder`."));
        assert!(prompt.contains("This model id is authoritative."));
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
