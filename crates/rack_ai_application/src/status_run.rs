use serde::Serialize;

use rack_ai_domain::RunState;
use rack_ai_domain::RunStatus;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StatusRun {
    task_id: String,
    status: String,
    attempt: u32,
    max_attempts: u32,
    admission_state: Option<String>,
    waiting_on_resources: Vec<String>,
    queue_path: Option<String>,
    result_path: Option<String>,
    last_error: Option<String>,
    submitted_at: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    active_node_id: Option<String>,
}

impl StatusRun {
    pub fn from_run_state(run_state: &RunState) -> Self {
        Self {
            task_id: run_state.task_id().value().to_string(),
            status: status_text(run_state.status()).to_string(),
            attempt: run_state.attempt_count().value(),
            max_attempts: run_state.attempt_limit().value(),
            admission_state: run_state.metadata().admission_state().cloned(),
            waiting_on_resources: run_state.metadata().waiting_on_resources().to_vec(),
            queue_path: run_state.metadata().queue_path().cloned(),
            result_path: run_state.metadata().result_path().cloned(),
            last_error: run_state.metadata().last_error().cloned(),
            submitted_at: run_state.metadata().submitted_at().cloned(),
            started_at: run_state.metadata().started_at().cloned(),
            finished_at: run_state.metadata().finished_at().cloned(),
            active_node_id: run_state
                .active_node_id()
                .map(|value| value.value().to_string()),
        }
    }
}

fn status_text(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Queued => "queued",
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::TimedOut => "timed_out",
        RunStatus::Blocked => "blocked",
    }
}
