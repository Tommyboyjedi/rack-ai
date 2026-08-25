use rack_ai_application::ChangeImplementer;
use rack_ai_application::ImplementChangeRequest;
use rack_ai_application::ImplementChangeResult;
use rack_ai_application::ImplementWorkerRuntime;
use rack_ai_application::ToolCallRecord;

use crate::JCodeProcessFailure;
use crate::JCodeProcessRunner;
use crate::JCodeWorkerConfigResolver;
use crate::RegistryPaths;

pub struct JCodeChangeImplementer {
    resolver: JCodeWorkerConfigResolver,
    default_worker: Option<ImplementWorkerRuntime>,
}

impl JCodeChangeImplementer {
    pub fn new(paths: RegistryPaths, default_worker: Option<ImplementWorkerRuntime>) -> Self {
        Self {
            resolver: JCodeWorkerConfigResolver::new(paths),
            default_worker,
        }
    }

    fn resolve_runtime(
        &self,
        request: &ImplementChangeRequest,
    ) -> Result<ImplementWorkerRuntime, String> {
        if let Some(runtime) = request.worker() {
            let resolved = self.resolver.resolve(runtime.worker_id())?;
            assert_runtime_matches(runtime, &resolved)?;
            return Ok(resolved);
        }
        self.default_worker
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.resolver.resolve_default_implementer())
    }
}

impl ChangeImplementer for JCodeChangeImplementer {
    fn implement(&self, request: &ImplementChangeRequest) -> Result<ImplementChangeResult, String> {
        let runtime = self.resolve_runtime(request)?;
        let output = JCodeProcessRunner::run_with_allowed_paths(
            &runtime,
            request.task(),
            request.worktree_path(),
            request.timeout_seconds(),
            request.network_disabled(),
            request.allowed_paths()?,
        );
        match output {
            Ok(result) => {
                let tool_calls = extract_jcode_tool_calls(result.stdout());
                Ok(
                    ImplementChangeResult::new(result.stdout().trim().to_string())
                        .with_stderr(result.stderr().to_string())
                        .with_tool_calls(tool_calls)
                        .with_executor_kind("jcode-direct".to_string()),
                )
            }
            Err(error) => Ok(build_failure_result(error)),
        }
    }
}

fn build_failure_result(error: JCodeProcessFailure) -> ImplementChangeResult {
    let tool_calls = extract_jcode_tool_calls(error.stdout());
    ImplementChangeResult::new(error.stdout().trim().to_string())
        .with_stderr(error.stderr().to_string())
        .with_tool_calls(tool_calls)
        .with_executor_kind("jcode-direct".to_string())
        .with_worker_error(error.message().to_string())
}

fn extract_jcode_tool_calls(output: &str) -> Vec<ToolCallRecord> {
    let lines: Vec<&str> = output.lines().collect();
    let mut calls = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some(close) = trimmed.find(']') else {
            continue;
        };

        if !trimmed.starts_with('[') || close <= 1 {
            continue;
        }

        let name = &trimmed[1..close];
        if !name
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_')
        {
            continue;
        }

        let has_token_record = lines
            .iter()
            .skip(index + 1)
            .take(3)
            .map(|line| line.trim())
            .find(|line| !line.is_empty())
            .is_some_and(|line| line.starts_with("[Tokens]"));

        if !has_token_record {
            continue;
        }

        calls.push(ToolCallRecord {
            name: name.to_string(),
            arguments: trimmed[close + 1..].trim().to_string(),
            result: String::new(),
        });
    }

    calls
}

fn assert_runtime_matches(
    request: &ImplementWorkerRuntime,
    resolved: &ImplementWorkerRuntime,
) -> Result<(), String> {
    if resolved.entrypoint() != request.entrypoint() {
        return Err(format!(
            "worker entrypoint mismatch for {}: request={}, registry={}",
            request.worker_id(),
            request.entrypoint(),
            resolved.entrypoint()
        ));
    }
    if resolved.provider_profile() != request.provider_profile() {
        return Err(format!(
            "worker provider profile mismatch for {}: request={}, registry={}",
            request.worker_id(),
            request.provider_profile(),
            resolved.provider_profile()
        ));
    }
    if resolved.api_model_id() != request.api_model_id() {
        return Err(format!(
            "worker model mismatch for {}: request={}, registry={}",
            request.worker_id(),
            request.api_model_id(),
            resolved.api_model_id()
        ));
    }
    if resolved.endpoint() != request.endpoint() {
        return Err(format!(
            "worker endpoint mismatch for {}: request={}, registry={}",
            request.worker_id(),
            request.endpoint(),
            resolved.endpoint()
        ));
    }
    if resolved.tool_profile() != request.tool_profile() {
        return Err(format!(
            "worker tool profile mismatch for {}: request={:?}, registry={:?}",
            request.worker_id(),
            request.tool_profile(),
            resolved.tool_profile()
        ));
    }
    if resolved.context_window() != request.context_window() {
        return Err(format!(
            "worker context window mismatch for {}: request={:?}, registry={:?}",
            request.worker_id(),
            request.context_window(),
            resolved.context_window()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::ChangeImplementer;
    use rack_ai_application::ImplementChangeRequest;
    use rack_ai_application::ImplementWorkerRuntime;

    use super::extract_jcode_tool_calls;
    use super::JCodeChangeImplementer;
    use crate::RegistryPaths;

    #[test]
    fn extracts_real_jcode_trace_tool_calls_only() {
        let output = r#"[read] src/lib.rs

[Tokens] upload: 100 download: 20

[write] src/store.rs

[Tokens] upload: 120 download: 30

[Tokens] upload: 140 download: 10

```rust
[write] fake.rs
```
"#;

        let calls = extract_jcode_tool_calls(output);

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "read");
        assert_eq!(calls[0].arguments, "src/lib.rs");
        assert_eq!(calls[1].name, "write");
        assert_eq!(calls[1].arguments, "src/store.rs");
    }

    #[test]
    fn rejects_request_runtime_context_mismatch() {
        let root = temp_root();
        write_registry(&root);
        let implementer = JCodeChangeImplementer::new(RegistryPaths::new(root), None);
        let worktree = temp_root();
        let request = ImplementChangeRequest::new(worktree, "task".to_string()).with_worker(
            ImplementWorkerRuntime::new(
                "local-coder".to_string(),
                "/home/tomp/.local/bin/jcode".to_string(),
                "local-coder".to_string(),
                "local-coder".to_string(),
                "http://127.0.0.1:8018/v1".to_string(),
            )
            .with_tool_profile(Some("minimal".to_string()))
            .with_context_window(Some(8192)),
        );

        let error = implementer.implement(&request).unwrap_err();

        assert!(error.contains("context window mismatch"));
    }

    #[test]
    fn rejects_request_runtime_model_mismatch() {
        let root = temp_root();
        write_registry(&root);
        let implementer = JCodeChangeImplementer::new(RegistryPaths::new(root), None);
        let worktree = temp_root();
        let request = ImplementChangeRequest::new(worktree, "task".to_string()).with_worker(
            ImplementWorkerRuntime::new(
                "local-coder".to_string(),
                "/home/tomp/.local/bin/jcode".to_string(),
                "local-coder".to_string(),
                "wrong-model".to_string(),
                "http://127.0.0.1:8018/v1".to_string(),
            )
            .with_tool_profile(Some("minimal".to_string()))
            .with_context_window(Some(16368)),
        );

        let error = implementer.implement(&request).unwrap_err();

        assert!(error.contains("worker model mismatch"));
    }

    fn write_registry(root: &std::path::PathBuf) {
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/workers.json"),
            r#"{
  "workers": [
    {
      "id": "local-coder",
      "kind": "jcode",
      "role": "implementer-tester",
      "entrypoint": "/home/tomp/.local/bin/jcode",
      "backend": "jcode",
      "resource_id": "gpu-2060",
      "model_id": "eqaq-v2-local-coder",
      "enabled": true,
      "provider_profile": "local-coder",
      "tool_profile": "minimal"
    }
  ]
}"#,
        )
        .unwrap();
        fs::write(
            root.join("config/models.json"),
            r#"{
  "models": [
    {
      "id": "eqaq-v2-local-coder",
      "label": "NotaMG/eqaq-v2",
      "role": "implementer",
      "backend": "vllm",
      "worker_id": "local-coder",
      "api_model_id": "local-coder",
      "context_window": 16368,
      "endpoint": "http://127.0.0.1:8018/v1",
      "port": 8018,
      "status": "active"
    }
  ]
}"#,
        )
        .unwrap();
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-jcode-implementer-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
