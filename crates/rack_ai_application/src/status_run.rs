use serde::Serialize;

use rack_ai_domain::DagRunState;
use rack_ai_domain::Placement;
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
    placement: Placement,
    active_node_id: Option<String>,
    #[serde(rename = "dag_state")]
    dag_run_state: Option<DagRunState>,
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
            placement: run_state.placement().clone(),
            active_node_id: run_state
                .active_node_id()
                .map(|value| value.value().to_string()),
            dag_run_state: run_state.dag_run_state().cloned(),
        }
    }

    pub fn task_id(&self) -> &str {
        self.task_id.as_str()
    }

    pub fn status(&self) -> &str {
        self.status.as_str()
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    pub fn admission_state(&self) -> Option<&String> {
        self.admission_state.as_ref()
    }

    pub fn waiting_on_resources(&self) -> &[String] {
        self.waiting_on_resources.as_slice()
    }

    pub fn active_node_id(&self) -> Option<&String> {
        self.active_node_id.as_ref()
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rack_ai_domain::ActiveNodeId;
    use rack_ai_domain::AttemptLimit;
    use rack_ai_domain::Placement;
    use rack_ai_domain::RunMetadata;
    use rack_ai_domain::RunState;
    use rack_ai_domain::RunStateDraft;
    use rack_ai_domain::TaskId;
    use rack_ai_domain::TimeoutSeconds;

    use super::StatusRun;

    #[test]
    fn serializes_run_metadata_for_status_output() {
        let run_state = RunState::queued(RunStateDraft {
            task_id: TaskId::new("task-77".to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(3).unwrap(),
            timeout_seconds: TimeoutSeconds::new(90).unwrap(),
            placement: Placement::new(
                vec!["local-coder".to_string()],
                vec!["gpu-2060".to_string()],
            ),
        })
        .with_metadata(
            RunMetadata::default()
                .submitted(
                    "2026-08-20T20:40:00Z".to_string(),
                    "/tmp/spec.json".to_string(),
                    "/state/queue/queued/task-77.json".to_string(),
                )
                .running(
                    "2026-08-20T20:40:05Z".to_string(),
                    "/state/queue/running/task-77.json".to_string(),
                    BTreeMap::from([(
                        "gpu-2060".to_string(),
                        "/state/resources/leases/gpu-2060.json".to_string(),
                    )]),
                ),
        )
        .start(Some(ActiveNodeId::new("implement".to_string()).unwrap()));

        let status_run = StatusRun::from_run_state(&run_state);
        let json = serde_json::to_string(&status_run).unwrap();

        assert!(json.contains("\"task_id\":\"task-77\""));
        assert!(json.contains("\"status\":\"running\""));
        assert!(json.contains("\"admission_state\":\"running\""));
        assert!(json.contains("\"queue_path\":\"/state/queue/running/task-77.json\""));
        assert!(json.contains("\"submitted_at\":\"2026-08-20T20:40:00Z\""));
        assert!(json.contains("\"started_at\":\"2026-08-20T20:40:05Z\""));
        assert!(json.contains("\"active_node_id\":\"implement\""));
        assert!(json.contains("\"placement\""));
    }
}
