use super::*;
pub(super) struct AttemptInput {
    pub selected: SelectedTask,
    pub started: String,
    pub leases: BTreeMap<String, String>,
}
pub(super) struct TaskAttempt<'a> {
    pub services: RunNextTaskDependencies<'a>,
    pub invoked: std::cell::Cell<bool>,
}
impl TaskAttempt<'_> {
    pub(super) fn execute_dag_task(&self, input: AttemptInput) -> Result<RunNextOutcome, String> {
        let AttemptInput {
            selected: selected_task,
            started: started_at,
            leases: lease_paths,
        } = input;
        let node_id = selected_task
            .active_node_id
            .clone()
            .ok_or("active dag node missing".to_string())?;
        let dag_run_state = selected_task
            .run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing".to_string())?
            .mark_running(&node_id, started_at.clone())?;
        let running_metadata = selected_task.run_state.metadata().clone().running(
            started_at,
            selected_task.queued_task.spec_path().to_string(),
            lease_paths,
        );
        let started_run_state = selected_task
            .run_state
            .start(Some(node_id.clone()))
            .with_dag_run_state(dag_run_state)
            .with_metadata(running_metadata);
        self.services
            .run_state_repository
            .save(&started_run_state)?;
        let execution_request = TaskExecutionRequest::new(
            selected_task.queued_task.task_id().to_string(),
            selected_task.queued_task.spec_path().to_string(),
        )
        .with_execution_spec_json(
            selected_task
                .task_spec
                .build_execution_spec_json(&node_id, &selected_task.placement)?,
        );
        self.invoked.set(true);
        let execution = self
            .services
            .task_executor
            .execute(&execution_request)
            .unwrap_or_else(|error| TaskExecution::failure(error, None));
        (resolution::DagResolution {
            services: self.services,
        })
        .resolve_dag_execution(
            resolution::ExecutionResult {
                queued_task: selected_task.queued_task,
                started_run_state,
                execution,
            },
            node_id,
        )
    }

    pub(super) fn execute_linear_task(
        &self,
        input: AttemptInput,
    ) -> Result<RunNextOutcome, String> {
        let AttemptInput {
            selected: selected_task,
            started: started_at,
            leases: lease_paths,
        } = input;
        let running_metadata = selected_task.run_state.metadata().clone().running(
            started_at,
            selected_task.queued_task.spec_path().to_string(),
            lease_paths,
        );
        let started_run_state = selected_task
            .run_state
            .start(None)
            .with_metadata(running_metadata);
        self.services
            .run_state_repository
            .save(&started_run_state)?;
        let execution_request = TaskExecutionRequest::new(
            selected_task.queued_task.task_id().to_string(),
            selected_task.queued_task.spec_path().to_string(),
        );
        self.invoked.set(true);
        let execution = self
            .services
            .task_executor
            .execute(&execution_request)
            .unwrap_or_else(|error| TaskExecution::failure(error, None));
        (resolution::LinearResolution {
            services: self.services,
        })
        .resolve_linear_execution(resolution::ExecutionResult {
            queued_task: selected_task.queued_task,
            started_run_state,
            execution,
        })
    }
}
