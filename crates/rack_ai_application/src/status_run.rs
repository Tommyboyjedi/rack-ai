use serde::Serialize;

use rack_ai_domain::RunState;
use rack_ai_domain::RunStatus;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StatusRun {
    task_id: String,
    status: String,
    attempt: u32,
    max_attempts: u32,
}

impl StatusRun {
    pub fn from_run_state(run_state: &RunState) -> Self {
        Self {
            task_id: run_state.task_id().value().to_string(),
            status: status_text(run_state.status()).to_string(),
            attempt: run_state.attempt_count().value(),
            max_attempts: run_state.attempt_limit().value(),
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
