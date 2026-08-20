use std::collections::BTreeMap;

use rack_ai_domain::ActiveNodeId;
use rack_ai_domain::Placement;
use rack_ai_domain::RunState;
use rack_ai_domain::TaskId;

use crate::Clock;
use crate::ExecutionQueueRepository;
use crate::LeaseRepository;
use crate::QueuedTask;
use crate::RunStateRepository;
use crate::TaskExecution;
use crate::TaskExecutionRequest;
use crate::TaskExecutor;
use crate::TaskSpec;
use crate::TaskSpecRepository;
use crate::WorkerCatalog;

pub struct RunNextTask<'a> {
    clock: &'a dyn Clock,
    execution_queue_repository: &'a dyn ExecutionQueueRepository,
    lease_repository: &'a dyn LeaseRepository,
    run_state_repository: &'a dyn RunStateRepository,
    task_executor: &'a dyn TaskExecutor,
    task_spec_repository: &'a dyn TaskSpecRepository,
    worker_catalog: &'a dyn WorkerCatalog,
}

pub struct RunNextTaskDependencies<'a> {
    pub clock: &'a dyn Clock,
    pub execution_queue_repository: &'a dyn ExecutionQueueRepository,
    pub lease_repository: &'a dyn LeaseRepository,
    pub run_state_repository: &'a dyn RunStateRepository,
    pub task_executor: &'a dyn TaskExecutor,
    pub task_spec_repository: &'a dyn TaskSpecRepository,
    pub worker_catalog: &'a dyn WorkerCatalog,
}

pub enum RunNextOutcome {
    NoQueuedTasks,
    NoAdmissibleTasks,
    Succeeded(String),
    Requeued(String),
    Failed(String),
}

struct SelectedTask {
    queued_task: QueuedTask,
    run_state: RunState,
    task_spec: TaskSpec,
    active_node_id: Option<ActiveNodeId>,
    placement: Placement,
}

enum Selection {
    NoneQueued,
    NoneAdmissible,
    Selected(SelectedTask),
}

impl<'a> RunNextTask<'a> {
    pub fn new(dependencies: RunNextTaskDependencies<'a>) -> Self {
        Self {
            clock: dependencies.clock,
            execution_queue_repository: dependencies.execution_queue_repository,
            lease_repository: dependencies.lease_repository,
            run_state_repository: dependencies.run_state_repository,
            task_executor: dependencies.task_executor,
            task_spec_repository: dependencies.task_spec_repository,
            worker_catalog: dependencies.worker_catalog,
        }
    }

    pub fn execute(&self) -> Result<RunNextOutcome, String> {
        let selection = self.select_task()?;
        let selected_task = match selection {
            Selection::NoneQueued => return Ok(RunNextOutcome::NoQueuedTasks),
            Selection::NoneAdmissible => return Ok(RunNextOutcome::NoAdmissibleTasks),
            Selection::Selected(value) => value,
        };
        let lease_paths = self
            .lease_repository
            .acquire(selected_task.run_state.task_id(), &selected_task.placement)?;
        if selected_task.task_spec.has_dag() {
            return self.execute_dag_task(selected_task, lease_paths);
        }
        self.execute_linear_task(selected_task, lease_paths)
    }

    fn execute_dag_task(
        &self,
        selected_task: SelectedTask,
        lease_paths: BTreeMap<String, String>,
    ) -> Result<RunNextOutcome, String> {
        let started_at = self.clock.now_text()?;
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
        self.run_state_repository.save(&started_run_state)?;
        let execution_request = TaskExecutionRequest::new(
            selected_task.queued_task.task_id().to_string(),
            selected_task.queued_task.spec_path().to_string(),
        )
        .with_execution_spec_json(
            selected_task
                .task_spec
                .build_execution_spec_json(&node_id, &selected_task.placement)?,
        );
        let execution = self
            .task_executor
            .execute(&execution_request)
            .unwrap_or_else(|error| TaskExecution::failure(error, None));
        self.resolve_dag_execution(
            selected_task.queued_task,
            selected_task.placement,
            started_run_state,
            node_id,
            execution,
        )
    }

    fn execute_linear_task(
        &self,
        selected_task: SelectedTask,
        lease_paths: BTreeMap<String, String>,
    ) -> Result<RunNextOutcome, String> {
        let started_at = self.clock.now_text()?;
        let running_metadata = selected_task.run_state.metadata().clone().running(
            started_at,
            selected_task.queued_task.spec_path().to_string(),
            lease_paths,
        );
        let started_run_state = selected_task
            .run_state
            .start(None)
            .with_metadata(running_metadata);
        self.run_state_repository.save(&started_run_state)?;
        let execution_request = TaskExecutionRequest::new(
            selected_task.queued_task.task_id().to_string(),
            selected_task.queued_task.spec_path().to_string(),
        );
        let execution = self
            .task_executor
            .execute(&execution_request)
            .unwrap_or_else(|error| TaskExecution::failure(error, None));
        self.resolve_linear_execution(
            selected_task.queued_task,
            selected_task.placement,
            started_run_state,
            execution,
        )
    }

    fn resolve_dag_execution(
        &self,
        queued_task: QueuedTask,
        placement: Placement,
        started_run_state: RunState,
        node_id: ActiveNodeId,
        execution: TaskExecution,
    ) -> Result<RunNextOutcome, String> {
        let finished_at = self.clock.now_text()?;
        let result = (|| {
            let dag_run_state = started_run_state
                .dag_run_state()
                .cloned()
                .ok_or("dag run state missing".to_string())?;
            if execution.was_successful() {
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
                    self.execution_queue_repository.complete(&queued_task)?;
                    self.run_state_repository.save(
                        &started_run_state
                            .succeed()
                            .with_dag_run_state(succeeded_dag_run_state)
                            .with_metadata(completed_metadata),
                    )?;
                    return Ok(RunNextOutcome::Succeeded(queued_task.task_id().to_string()));
                }
                let requeued_task = self.execution_queue_repository.requeue(&queued_task)?;
                let queued_metadata = started_run_state.metadata().clone().queued(
                    requeued_task.spec_path().to_string(),
                    finished_at.clone(),
                    execution.result_path().cloned(),
                    None,
                );
                self.run_state_repository.save(
                    &started_run_state
                        .queue()
                        .with_dag_run_state(succeeded_dag_run_state)
                        .with_metadata(queued_metadata),
                )?;
                return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
            }
            let last_error = execution
                .last_error()
                .cloned()
                .unwrap_or_else(|| "execution failed".to_string());
            if started_run_state.can_retry() {
                let pending_dag_run_state =
                    dag_run_state.mark_pending(&node_id, last_error.clone())?;
                let requeued_task = self.execution_queue_repository.requeue(&queued_task)?;
                let queued_metadata = started_run_state.metadata().clone().queued(
                    requeued_task.spec_path().to_string(),
                    finished_at.clone(),
                    execution.result_path().cloned(),
                    Some(last_error),
                );
                self.run_state_repository.save(
                    &started_run_state
                        .queue()
                        .with_dag_run_state(pending_dag_run_state)
                        .with_metadata(queued_metadata),
                )?;
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
            self.execution_queue_repository.complete(&queued_task)?;
            self.run_state_repository.save(
                &started_run_state
                    .fail()
                    .with_dag_run_state(failed_dag_run_state)
                    .with_metadata(failed_metadata),
            )?;
            Ok(RunNextOutcome::Failed(queued_task.task_id().to_string()))
        })();
        self.lease_repository.release(&placement)?;
        result
    }

    fn resolve_linear_execution(
        &self,
        queued_task: QueuedTask,
        placement: Placement,
        started_run_state: RunState,
        execution: TaskExecution,
    ) -> Result<RunNextOutcome, String> {
        let finished_at = self.clock.now_text()?;
        let result = (|| {
            if execution.was_successful() {
                let completed_metadata = started_run_state
                    .metadata()
                    .clone()
                    .completed(finished_at.clone(), execution.result_path().cloned());
                self.execution_queue_repository.complete(&queued_task)?;
                self.run_state_repository.save(
                    &started_run_state
                        .succeed()
                        .with_metadata(completed_metadata),
                )?;
                return Ok(RunNextOutcome::Succeeded(queued_task.task_id().to_string()));
            }
            let last_error = execution
                .last_error()
                .cloned()
                .unwrap_or_else(|| "execution failed".to_string());
            if started_run_state.can_retry() {
                let requeued_task = self.execution_queue_repository.requeue(&queued_task)?;
                let queued_metadata = started_run_state.metadata().clone().queued(
                    requeued_task.spec_path().to_string(),
                    finished_at.clone(),
                    execution.result_path().cloned(),
                    Some(last_error),
                );
                self.run_state_repository
                    .save(&started_run_state.queue().with_metadata(queued_metadata))?;
                return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
            }
            let failed_metadata = started_run_state.metadata().clone().failed(
                finished_at.clone(),
                execution.result_path().cloned(),
                last_error,
            );
            self.execution_queue_repository.complete(&queued_task)?;
            self.run_state_repository
                .save(&started_run_state.fail().with_metadata(failed_metadata))?;
            Ok(RunNextOutcome::Failed(queued_task.task_id().to_string()))
        })();
        self.lease_repository.release(&placement)?;
        result
    }

    fn select_task(&self) -> Result<Selection, String> {
        let queued_tasks = self.execution_queue_repository.list()?;
        if queued_tasks.is_empty() {
            return Ok(Selection::NoneQueued);
        }
        for queued_task in queued_tasks {
            let task_id = TaskId::new(queued_task.task_id().to_string())?;
            let run_state = self.load_run_state(&task_id)?;
            let task_spec = self.task_spec_repository.load(&queued_task)?;
            let (run_state, active_node_id, placement) = self.plan_task(run_state, &task_spec)?;
            let blocked = self.lease_repository.blocked_resources(&placement)?;
            if !blocked.is_empty() {
                let updated = run_state.clone().with_metadata(
                    run_state
                        .metadata()
                        .clone()
                        .waiting_for_resources(queued_task.spec_path().to_string(), blocked),
                );
                self.run_state_repository.save(&updated)?;
                continue;
            }
            let ready = run_state.clone().with_metadata(
                run_state
                    .metadata()
                    .clone()
                    .ready(queued_task.spec_path().to_string()),
            );
            self.run_state_repository.save(&ready)?;
            let claimed = self.execution_queue_repository.claim(&queued_task)?;
            return Ok(Selection::Selected(SelectedTask {
                queued_task: claimed,
                run_state,
                task_spec,
                active_node_id,
                placement,
            }));
        }
        Ok(Selection::NoneAdmissible)
    }

    fn plan_task(
        &self,
        run_state: RunState,
        task_spec: &TaskSpec,
    ) -> Result<(RunState, Option<ActiveNodeId>, Placement), String> {
        if !task_spec.has_dag() {
            return Ok((run_state, None, task_spec.placement().clone()));
        }
        let run_state = self.ensure_dag_run_state(run_state, task_spec)?;
        let dag_run_state = run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing".to_string())?;
        let node_id = task_spec
            .first_ready_node_id(&dag_run_state)
            .ok_or("no ready dag node available".to_string())?;
        let placement = task_spec.dag_node_placement(&node_id, self.worker_catalog)?;
        Ok((run_state, Some(node_id), placement))
    }

    fn ensure_dag_run_state(
        &self,
        run_state: RunState,
        task_spec: &TaskSpec,
    ) -> Result<RunState, String> {
        if run_state.dag_run_state().is_some() {
            return Ok(run_state);
        }
        let dag_run_state = task_spec
            .dag_run_state()?
            .ok_or("dag task was missing initial dag state".to_string())?;
        Ok(run_state.with_dag_run_state(dag_run_state))
    }

    fn load_run_state(&self, task_id: &TaskId) -> Result<RunState, String> {
        self.run_state_repository
            .find(task_id)?
            .ok_or("run state missing for queued task".to_string())
    }
}
