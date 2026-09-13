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

mod attempt;
mod planning;
mod resolution;
use crate::LeaseRequest;
#[derive(Clone, Copy)]
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

pub(super) struct SelectedTask {
    pub(super) queued_task: QueuedTask,
    pub(super) run_state: RunState,
    pub(super) task_spec: TaskSpec,
    pub(super) active_node_id: Option<ActiveNodeId>,
    pub(super) placement: Placement,
}

pub(super) enum Selection {
    NoneQueued,
    NoneAdmissible,
    Selected(SelectedTask),
}

pub struct RunNextTask<'a> {
    services: RunNextTaskDependencies<'a>,
}
impl<'a> RunNextTask<'a> {
    pub fn new(services: RunNextTaskDependencies<'a>) -> Self {
        Self { services }
    }
    pub fn execute(&self) -> Result<RunNextOutcome, String> {
        let selected = match (planning::TaskSelection {
            services: self.services,
        })
        .select_task()?
        {
            Selection::NoneQueued => return Ok(RunNextOutcome::NoQueuedTasks),
            Selection::NoneAdmissible => return Ok(RunNextOutcome::NoAdmissibleTasks),
            Selection::Selected(value) => value,
        };
        let queued = selected.queued_task.clone();
        let original = selected.run_state.clone();
        let acquired = self.services.clock.now_text().and_then(|started| {
            self.services
                .lease_repository
                .acquire(&LeaseRequest {
                    task_id: selected.run_state.task_id(),
                    placement: &selected.placement,
                    acquired_at: &started,
                })
                .map(|handle| (started, handle))
        });
        let (started, handle) = match acquired {
            Ok(value) => value,
            Err(error) => {
                self.services.execution_queue_repository.requeue(&queued)?;
                return Err(error);
            }
        };
        let attempt = attempt::TaskAttempt {
            services: self.services,
            invoked: std::cell::Cell::new(false),
        };
        let is_dag = selected.task_spec.has_dag();
        let input = attempt::AttemptInput {
            selected,
            started,
            leases: handle.paths.clone(),
        };
        let result = if is_dag {
            attempt.execute_dag_task(input)
        } else {
            attempt.execute_linear_task(input)
        };
        let released = self.services.lease_repository.release(&handle);
        if result.is_err() && !attempt.invoked.get() {
            let queued = self.services.execution_queue_repository.requeue(&queued)?;
            let metadata = original.metadata().clone().queued(
                queued.spec_path().to_owned(),
                self.services.clock.now_text()?,
                None,
                Some("preparation failed before invocation".into()),
            );
            self.services
                .run_state_repository
                .save(&original.queue().with_metadata(metadata))?;
        }
        // Once invoked, persistence uncertainty retains the claimed record for operator
        // reconciliation; it must never schedule a second execution automatically.
        released?;
        result
    }
}

#[cfg(test)]
mod tests;
