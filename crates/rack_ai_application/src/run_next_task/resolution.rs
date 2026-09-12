use super::*;
pub(super) struct ExecutionResult {
    pub queued_task: QueuedTask,
    pub started_run_state: RunState,
    pub execution: TaskExecution,
}
pub(super) struct DagResult {
    result: ExecutionResult,
    node_id: ActiveNodeId,
    finished_at: String,
}
pub(super) struct DagResolution<'a> {
    pub services: RunNextTaskDependencies<'a>,
}
impl DagResolution<'_> {
    pub(super) fn resolve_dag_execution(
        &self,
        result: ExecutionResult,
        node_id: ActiveNodeId,
    ) -> Result<RunNextOutcome, String> {
        let finished_at = self.services.clock.now_text()?;
        let successful = result.execution.was_successful();
        let context = DagResult {
            result,
            node_id,
            finished_at,
        };
        if successful {
            DagSuccess {
                services: self.services,
            }
            .resolve(context)
        } else {
            DagFailure {
                services: self.services,
            }
            .resolve(context)
        }
    }
}
pub(super) struct DagSuccess<'a> {
    pub services: RunNextTaskDependencies<'a>,
}
impl DagSuccess<'_> {
    fn resolve(&self, context: DagResult) -> Result<RunNextOutcome, String> {
        let DagResult {
            result,
            node_id,
            finished_at,
        } = context;
        let ExecutionResult {
            queued_task,
            started_run_state,
            execution,
        } = result;
        let dag_run_state = started_run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing")?;
        let succeeded_dag_run_state = dag_run_state.mark_succeeded(
            &node_id,
            finished_at.clone(),
            execution.result_path().cloned(),
        )?;
        if succeeded_dag_run_state.all_succeeded() {
            let completed_metadata = started_run_state
                .metadata()
                .clone()
                .completed(finished_at.clone(), execution.result_path().cloned());
            self.services.run_state_repository.save(
                &started_run_state
                    .succeed()
                    .with_dag_run_state(succeeded_dag_run_state)
                    .with_metadata(completed_metadata),
            )?;
            self.services
                .execution_queue_repository
                .complete(&queued_task)?;
            return Ok(RunNextOutcome::Succeeded(queued_task.task_id().to_string()));
        }
        let queued_metadata = started_run_state.metadata().clone().queued(
            queued_task.spec_path().to_string(),
            finished_at.clone(),
            execution.result_path().cloned(),
            None,
        );
        self.services.run_state_repository.save(
            &started_run_state
                .queue()
                .with_dag_run_state(succeeded_dag_run_state)
                .with_metadata(queued_metadata),
        )?;
        self.services
            .execution_queue_repository
            .requeue(&queued_task)?;
        return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
    }
}
pub(super) struct DagFailure<'a> {
    pub services: RunNextTaskDependencies<'a>,
}
impl DagFailure<'_> {
    fn resolve(&self, context: DagResult) -> Result<RunNextOutcome, String> {
        let DagResult {
            result,
            node_id,
            finished_at,
        } = context;
        let ExecutionResult {
            queued_task,
            started_run_state,
            execution,
        } = result;
        let dag_run_state = started_run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing")?;
        let last_error = execution
            .last_error()
            .cloned()
            .unwrap_or_else(|| "execution failed".to_string());
        if started_run_state.can_retry() {
            let pending_dag_run_state = dag_run_state.mark_pending(&node_id, last_error.clone())?;
            let queued_metadata = started_run_state.metadata().clone().queued(
                queued_task.spec_path().to_string(),
                finished_at.clone(),
                execution.result_path().cloned(),
                Some(last_error),
            );
            self.services.run_state_repository.save(
                &started_run_state
                    .queue()
                    .with_dag_run_state(pending_dag_run_state)
                    .with_metadata(queued_metadata),
            )?;
            self.services
                .execution_queue_repository
                .requeue(&queued_task)?;
            return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
        }
        let failed_dag_run_state = dag_run_state.mark_failed(
            &node_id,
            finished_at.clone(),
            execution.result_path().cloned(),
            last_error.clone(),
        )?;
        let failed_metadata = started_run_state.metadata().clone().failed(
            finished_at.clone(),
            execution.result_path().cloned(),
            last_error,
        );
        self.services.run_state_repository.save(
            &started_run_state
                .fail()
                .with_dag_run_state(failed_dag_run_state)
                .with_metadata(failed_metadata),
        )?;
        self.services
            .execution_queue_repository
            .complete(&queued_task)?;
        Ok(RunNextOutcome::Failed(queued_task.task_id().to_string()))
    }
}
pub(super) struct LinearResolution<'a> {
    pub services: RunNextTaskDependencies<'a>,
}
impl LinearResolution<'_> {
    pub(super) fn resolve_linear_execution(
        &self,
        result: ExecutionResult,
    ) -> Result<RunNextOutcome, String> {
        let ExecutionResult {
            queued_task,
            started_run_state,
            execution,
        } = result;
        let finished_at = self.services.clock.now_text()?;
        let result = (|| {
            if execution.was_successful() {
                let completed_metadata = started_run_state
                    .metadata()
                    .clone()
                    .completed(finished_at.clone(), execution.result_path().cloned());
                self.services.run_state_repository.save(
                    &started_run_state
                        .succeed()
                        .with_metadata(completed_metadata),
                )?;
                self.services
                    .execution_queue_repository
                    .complete(&queued_task)?;
                return Ok(RunNextOutcome::Succeeded(queued_task.task_id().to_string()));
            }
            let last_error = execution
                .last_error()
                .cloned()
                .unwrap_or_else(|| "execution failed".to_string());
            if started_run_state.can_retry() {
                let queued_metadata = started_run_state.metadata().clone().queued(
                    queued_task.spec_path().to_string(),
                    finished_at.clone(),
                    execution.result_path().cloned(),
                    Some(last_error),
                );
                self.services
                    .run_state_repository
                    .save(&started_run_state.queue().with_metadata(queued_metadata))?;
                self.services
                    .execution_queue_repository
                    .requeue(&queued_task)?;
                return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
            }
            let failed_metadata = started_run_state.metadata().clone().failed(
                finished_at.clone(),
                execution.result_path().cloned(),
                last_error,
            );
            self.services
                .run_state_repository
                .save(&started_run_state.fail().with_metadata(failed_metadata))?;
            self.services
                .execution_queue_repository
                .complete(&queued_task)?;
            Ok(RunNextOutcome::Failed(queued_task.task_id().to_string()))
        })();
        result
    }
}
