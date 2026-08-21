use std::sync::Mutex;

use serde::Deserialize;
use serde::Serialize;

use crate::ChangeImplementer;
use crate::ImplementChangeRequest;
use crate::ImplementChangeResult;
use crate::ToolCallRecord;
use crate::WorkspaceExecutor;
use crate::WorkspacePath;
use crate::WriteFileRequest;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScriptedWrite {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScriptedAttempt {
    #[serde(default)]
    pub match_worker: Option<String>,
    #[serde(default)]
    pub writes: Vec<ScriptedWrite>,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub protocol_error: Option<String>,
    #[serde(default)]
    pub executor_kind: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScriptedImplementerDocument {
    pub attempts: Vec<ScriptedAttempt>,
}

pub struct ScriptedChangeImplementer<'a> {
    executor: &'a dyn WorkspaceExecutor,
    remaining: Mutex<Vec<ScriptedAttempt>>,
    seen_workers: Mutex<Vec<String>>,
}

impl<'a> ScriptedChangeImplementer<'a> {
    pub fn new(executor: &'a dyn WorkspaceExecutor, attempts: Vec<ScriptedAttempt>) -> Self {
        Self {
            executor,
            remaining: Mutex::new(attempts),
            seen_workers: Mutex::new(Vec::new()),
        }
    }

    pub fn from_document(
        executor: &'a dyn WorkspaceExecutor,
        document: ScriptedImplementerDocument,
    ) -> Self {
        Self::new(executor, document.attempts)
    }

    pub fn seen_workers(&self) -> Vec<String> {
        self.seen_workers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl ChangeImplementer for ScriptedChangeImplementer<'_> {
    fn implement(&self, request: &ImplementChangeRequest) -> Result<ImplementChangeResult, String> {
        let mut remaining = self.remaining.lock().map_err(|error| error.to_string())?;
        let spec = remaining
            .drain(0..1)
            .next()
            .ok_or_else(|| "scripted implementer has no remaining attempts".to_string())?;
        drop(remaining);
        let worker_id = request.worker_id().unwrap_or("unspecified").to_string();
        self.seen_workers
            .lock()
            .map_err(|error| error.to_string())?
            .push(worker_id.clone());
        if let Some(expected) = &spec.match_worker {
            if expected != &worker_id {
                return Err(format!(
                    "scripted implementer expected worker {expected}, received {worker_id}"
                ));
            }
        }
        if let Some(error) = spec.error {
            return Err(error);
        }
        let executor_kind = spec
            .executor_kind
            .clone()
            .unwrap_or_else(|| "workspace".to_string());
        if executor_kind == "host" || executor_kind == "jcode" {
            return Ok(ImplementChangeResult::new(spec.output)
                .with_executor_kind(executor_kind)
                .with_protocol_error("host-shell executor used".to_string()));
        }
        let mut tool_calls = Vec::new();
        for write in spec.writes {
            let path = WorkspacePath::parse(write.path.as_str())?;
            self.executor.write_file(
                &WriteFileRequest::new(request.worktree_path().to_path_buf(), path)
                    .with_content(write.content.clone()),
            )?;
            tool_calls.push(ToolCallRecord {
                name: "write".to_string(),
                arguments: format!("{{\"file_path\":\"{}\"}}", write.path),
                result: "ok".to_string(),
            });
        }
        let mut result = ImplementChangeResult::new(if spec.output.is_empty() {
            "COMPLETE".to_string()
        } else {
            spec.output
        })
        .with_tool_calls(tool_calls)
        .with_executor_kind(executor_kind);
        if let Some(protocol_error) = spec.protocol_error {
            result = result.with_protocol_error(protocol_error);
        }
        Ok(result)
    }
}
