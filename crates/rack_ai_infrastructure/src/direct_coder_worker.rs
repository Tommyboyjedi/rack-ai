use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use rack_ai_application::CoderRunRequest;
use rack_ai_application::CoderToolRunner;
use serde_json::Value;
use serde_json::json;

use crate::HostCoderToolRunner;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(330);
const MAX_MODEL_TURN_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_MODEL_RESPONSE_TOKENS: u32 = 3072;
const MAX_CONSECUTIVE_CORRECTABLE_TOOL_ERRORS: usize = 3;
const TOOL_ARGUMENT_ERROR_PREFIX: &str = "missing string field: ";

pub struct DirectCoderWorker {
    endpoint: String,
    model_id: String,
    system_prompt: String,
}

trait ChatCompletionClient {
    fn complete(&self, messages: &[Value], request_timeout: Duration) -> Result<Value, String>;
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
        self.execute_with_client(request, runner, self)
    }

    fn execute_with_client<C: ChatCompletionClient>(
        &self,
        request: &CoderRunRequest,
        runner: &dyn CoderToolRunner,
        client: &C,
    ) -> Result<String, String> {
        let mut messages = vec![
            json!({"role": "system", "content": self.system_prompt}),
            json!({"role": "user", "content": self.build_prompt(request.task())}),
        ];
        let deadline = request
            .timeout_seconds()
            .map(|seconds| Instant::now() + Duration::from_secs(u64::from(seconds.max(1))));
        let mut consecutive_tool_errors = 0usize;
        for _ in 0..request.max_turns() {
            if let Some(deadline) = deadline {
                if Instant::now() >= deadline {
                    return Err("coder wall-clock timeout exceeded".to_string());
                }
            }
            let request_timeout = deadline
                .map(|limit| limit.saturating_duration_since(Instant::now()))
                .unwrap_or(DEFAULT_REQUEST_TIMEOUT);
            if request_timeout.is_zero() {
                return Err("coder wall-clock timeout exceeded".to_string());
            }
            let response = client.complete(&messages, request_timeout)?;
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
                match runner.run(name, &parsed_arguments) {
                    Ok(result) => {
                        consecutive_tool_errors = 0;
                        messages.push(tool_result_message(tool_call_id, result));
                    }
                    Err(error) => {
                        if !is_correctable_tool_invocation_error(name, &error) {
                            return Err(error);
                        }
                        consecutive_tool_errors += 1;
                        if consecutive_tool_errors >= MAX_CONSECUTIVE_CORRECTABLE_TOOL_ERRORS {
                            return Err(format!(
                                "tool correction limit exceeded after {} consecutive invocation errors: {}",
                                MAX_CONSECUTIVE_CORRECTABLE_TOOL_ERRORS, error
                            ));
                        }
                        messages.push(tool_result_message(
                            tool_call_id,
                            format!(
                                "error: {error}\nCorrect the arguments for the same tool and retry with a valid tool call. Do not respond with prose or COMPLETE until the tool succeeds."
                            ),
                        ));
                    }
                }
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
    - `write` requires BOTH `file_path` and `content`.\n\
    - `read` requires `file_path`.\n\
    - `bash` requires `command`.\n\
    - If a tool returns an argument or tool error, correct the arguments and retry with another real tool call.\n\
    - Do not merely describe the correction in prose.\n\
    - Do not answer COMPLETE until the required filesystem changes have actually succeeded.\n\
    - After the requested file action is confirmed, reply with exactly COMPLETE and stop.\n\
    - Do not write the word COMPLETE into any project file unless explicitly asked.\n\n\
    Task:\n{}",
            self.model_id, task
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
            "max_tokens": DEFAULT_MODEL_RESPONSE_TOKENS,
        });

        let turn_timeout = request_timeout.min(MAX_MODEL_TURN_TIMEOUT);

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

impl ChatCompletionClient for DirectCoderWorker {
    fn complete(&self, messages: &[Value], request_timeout: Duration) -> Result<Value, String> {
        self.call_api(messages, request_timeout)
    }
}

fn tool_result_message(tool_call_id: &str, content: String) -> Value {
    json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content,
    })
}

fn is_correctable_tool_invocation_error(name: &str, error: &str) -> bool {
    matches!(name, "write" | "read" | "bash") && error.starts_with(TOOL_ARGUMENT_ERROR_PREFIX)
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
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use rack_ai_application::CoderRunRequest;
    use rack_ai_application::CoderWorkspaceContext;
    use rack_ai_domain::AllowedPath;
    use rack_ai_domain::AllowedPaths;
    use serde_json::Value;
    use serde_json::json;

    use super::ChatCompletionClient;
    use super::DirectCoderWorker;
    use super::HostCoderToolRunner;
    use crate::WorkspaceCoderToolRunner;

    #[test]
    fn builds_completion_prompt() {
        let worker = DirectCoderWorker::local_default();
        let prompt = worker.build_prompt("Write file");
        assert!(prompt.contains("Use actual tool calls."));
        assert!(prompt.contains("Task:\nWrite file"));
        assert!(prompt.contains("Your assigned model id is `local-coder`."));
        assert!(prompt.contains("This model id is authoritative."));
        assert!(prompt.contains("`write` requires BOTH `file_path` and `content`."));
        assert!(prompt.contains("correct the arguments and retry"));
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

    #[test]
    fn retries_correctable_write_error_within_same_execution() {
        let worker = DirectCoderWorker::local_default();
        let cwd = temp_root();
        let runner = HostCoderToolRunner::new(cwd.clone());
        let client = ScriptedClient::new(vec![
            tool_call_response(
                "call-1",
                "write",
                json!({"content": "pub fn value() -> u8 { 1 }\n"}),
            ),
            tool_call_response(
                "call-2",
                "write",
                json!({"file_path": "src/value.rs", "content": "pub fn value() -> u8 { 1 }\n"}),
            ),
            stop_response("COMPLETE"),
        ]);
        let output = worker
            .execute_with_client(
                &CoderRunRequest::new("Write src/value.rs".to_string(), 6).unwrap(),
                &runner,
                &client,
            )
            .unwrap();
        assert_eq!(output, "COMPLETE");
        assert_eq!(
            fs::read_to_string(cwd.join("src/value.rs")).unwrap(),
            "pub fn value() -> u8 { 1 }\n"
        );
        assert!(conversation_contains(
            &client.message_log(),
            "missing string field: file_path"
        ));
    }

    #[test]
    fn stops_after_bounded_consecutive_correctable_errors() {
        let worker = DirectCoderWorker::local_default();
        let cwd = temp_root();
        let runner = HostCoderToolRunner::new(cwd);
        let client = ScriptedClient::new(vec![
            tool_call_response("call-1", "write", json!({"content": "one"})),
            tool_call_response("call-2", "write", json!({"content": "two"})),
            tool_call_response("call-3", "write", json!({"content": "three"})),
        ]);
        let error = worker
            .execute_with_client(
                &CoderRunRequest::new("Write file".to_string(), 6).unwrap(),
                &runner,
                &client,
            )
            .unwrap_err();
        assert!(error.contains("tool correction limit exceeded"));
        assert_eq!(client.call_count(), 3);
    }

    #[test]
    fn valid_tool_calls_still_work_through_the_conversation_loop() {
        let worker = DirectCoderWorker::local_default();
        let cwd = temp_root();
        let runner = HostCoderToolRunner::new(cwd.clone());
        let client = ScriptedClient::new(vec![
            tool_call_response(
                "call-1",
                "write",
                json!({"file_path": "nested/out.txt", "content": "hello\n"}),
            ),
            tool_call_response("call-2", "read", json!({"file_path": "nested/out.txt"})),
            tool_call_response("call-3", "bash", json!({"command": "cat nested/out.txt"})),
            stop_response("COMPLETE"),
        ]);
        let output = worker
            .execute_with_client(
                &CoderRunRequest::new("Write, read, and inspect a file".to_string(), 8).unwrap(),
                &runner,
                &client,
            )
            .unwrap();
        assert_eq!(output, "COMPLETE");
        assert_eq!(
            fs::read_to_string(cwd.join("nested/out.txt")).unwrap(),
            "hello\n"
        );
        let messages = client.message_log();
        assert!(conversation_contains(&messages, "nested/out.txt"));
        assert!(conversation_contains(&messages, "1\thello"));
    }

    #[test]
    fn path_policy_violations_remain_terminal() {
        let worker = DirectCoderWorker::local_default();
        let runner = WorkspaceCoderToolRunner::new(
            &NoopWorkspaceExecutor,
            CoderWorkspaceContext::new(
                PathBuf::from("/tmp/repo"),
                AllowedPaths::new(vec![AllowedPath::new("src".to_string()).unwrap()]).unwrap(),
            ),
        );
        let client = ScriptedClient::new(vec![tool_call_response(
            "call-1",
            "write",
            json!({"file_path": "README.md", "content": "pwned\n"}),
        )]);
        let error = worker
            .execute_with_client(
                &CoderRunRequest::new("Write README.md".to_string(), 4).unwrap(),
                &runner,
                &client,
            )
            .unwrap_err();
        assert!(error.contains("outside allowed_paths"));
        assert_eq!(client.call_count(), 1);
    }

    struct ScriptedClient {
        responses: RefCell<VecDeque<Value>>,
        seen_messages: RefCell<Vec<Vec<Value>>>,
    }

    impl ScriptedClient {
        fn new(responses: Vec<Value>) -> Self {
            Self {
                responses: RefCell::new(VecDeque::from(responses)),
                seen_messages: RefCell::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.seen_messages.borrow().len()
        }

        fn message_log(&self) -> Vec<Vec<Value>> {
            self.seen_messages.borrow().clone()
        }
    }

    impl ChatCompletionClient for ScriptedClient {
        fn complete(
            &self,
            messages: &[Value],
            _request_timeout: Duration,
        ) -> Result<Value, String> {
            self.seen_messages.borrow_mut().push(messages.to_vec());
            self.responses
                .borrow_mut()
                .pop_front()
                .ok_or("scripted client exhausted".to_string())
        }
    }

    struct NoopWorkspaceExecutor;

    impl rack_ai_application::WorkspaceExecutor for NoopWorkspaceExecutor {
        fn write_file(
            &self,
            _request: &rack_ai_application::WriteFileRequest,
        ) -> Result<rack_ai_application::WorkspaceExecutionResult, String> {
            Err("write should not reach executor".to_string())
        }

        fn read_file(
            &self,
            _request: &rack_ai_application::ReadFileRequest,
        ) -> Result<rack_ai_application::WorkspaceExecutionResult, String> {
            Err("read should not reach executor".to_string())
        }

        fn run_command(
            &self,
            _request: &rack_ai_application::RunCommandRequest,
        ) -> Result<rack_ai_application::WorkspaceExecutionResult, String> {
            Err("bash should not reach executor".to_string())
        }
    }

    fn tool_call_response(id: &str, name: &str, arguments: Value) -> Value {
        json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments.to_string()
                        }
                    }]
                }
            }]
        })
    }

    fn stop_response(content: &str) -> Value {
        json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": content
                }
            }]
        })
    }

    fn conversation_contains(messages: &[Vec<Value>], fragment: &str) -> bool {
        messages.iter().flatten().any(|message| {
            message.get("role").and_then(Value::as_str) == Some("tool")
                && message
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .contains(fragment)
        })
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-direct-coder-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
