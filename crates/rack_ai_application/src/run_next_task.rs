use rack_ai_domain::ActiveNodeId;
use rack_ai_domain::Placement;
use rack_ai_domain::RunState;
use rack_ai_domain::TaskId;

use crate::ExecutionQueueRepository;
use crate::LeaseRepository;
use crate::QueuedTask;
use crate::RunStateRepository;
use crate::TaskExecutionRequest;
use crate::TaskExecutor;
use crate::TaskSpec;
use crate::TaskSpecRepository;

pub struct RunNextTask<'a> {
    execution_queue_repository: &'a dyn ExecutionQueueRepository,
    lease_repository: &'a dyn LeaseRepository,
    run_state_repository: &'a dyn RunStateRepository,
    task_executor: &'a dyn TaskExecutor,
    task_spec_repository: &'a dyn TaskSpecRepository,
}

pub struct RunNextTaskDependencies<'a> {
    pub execution_queue_repository: &'a dyn ExecutionQueueRepository,
    pub lease_repository: &'a dyn LeaseRepository,
    pub run_state_repository: &'a dyn RunStateRepository,
    pub task_executor: &'a dyn TaskExecutor,
    pub task_spec_repository: &'a dyn TaskSpecRepository,
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

impl<'a> RunNextTask<'a> {
    pub fn new(dependencies: RunNextTaskDependencies<'a>) -> Self {
        Self {
            execution_queue_repository: dependencies.execution_queue_repository,
            lease_repository: dependencies.lease_repository,
            run_state_repository: dependencies.run_state_repository,
            task_executor: dependencies.task_executor,
            task_spec_repository: dependencies.task_spec_repository,
        }
    }

    pub fn execute(&self) -> Result<RunNextOutcome, String> {
        let selection = self.select_task()?;
        let selected_task = match selection {
            Selection::NoneQueued => return Ok(RunNextOutcome::NoQueuedTasks),
            Selection::NoneAdmissible => return Ok(RunNextOutcome::NoAdmissibleTasks),
            Selection::Selected(value) => value,
        };
        self.lease_repository
            .acquire(selected_task.run_state.task_id(), &selected_task.placement)?;
        let result = if selected_task.task_spec.has_dag() {
            self.execute_dag_task(selected_task)
        } else {
            self.execute_linear_task(selected_task)
        };
        result
    }

    fn execute_dag_task(&self, selected_task: SelectedTask) -> Result<RunNextOutcome, String> {
        let dag_run_state = selected_task
            .run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing".to_string())?
            .mark_running(
                selected_task
                    .active_node_id
                    .as_ref()
                    .ok_or("active dag node missing".to_string())?,
            )?;
        let started_run_state = selected_task
            .run_state
            .start(selected_task.active_node_id.clone())
            .with_dag_run_state(dag_run_state);
        self.run_state_repository.save(&started_run_state)?;
        let node_id = selected_task
            .active_node_id
            .ok_or("active dag node missing".to_string())?;
        let execution_request = TaskExecutionRequest::new(
            selected_task.queued_task.task_id().to_string(),
            selected_task.queued_task.spec_path().to_string(),
        )
        .with_execution_spec_json(
            selected_task
                .task_spec
                .build_execution_spec_json(&node_id)?,
        );
        let execution = self.task_executor.execute(&execution_request);
        let outcome = self.resolve_dag_execution(
            selected_task.queued_task,
            started_run_state,
            node_id,
            execution,
        );
        self.lease_repository.release(&selected_task.placement)?;
        outcome
    }

    fn execute_linear_task(&self, selected_task: SelectedTask) -> Result<RunNextOutcome, String> {
        let started_run_state = selected_task.run_state.start(None);
        self.run_state_repository.save(&started_run_state)?;
        let execution_request = TaskExecutionRequest::new(
            selected_task.queued_task.task_id().to_string(),
            selected_task.queued_task.spec_path().to_string(),
        );
        let execution = self.task_executor.execute(&execution_request);
        let outcome =
            self.resolve_linear_execution(selected_task.queued_task, started_run_state, execution);
        self.lease_repository.release(&selected_task.placement)?;
        outcome
    }

    fn resolve_dag_execution(
        &self,
        queued_task: QueuedTask,
        started_run_state: RunState,
        node_id: ActiveNodeId,
        execution: Result<crate::TaskExecution, String>,
    ) -> Result<RunNextOutcome, String> {
        let dag_run_state = started_run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing".to_string())?;
        let execution = execution?;
        if execution.was_successful() {
            let succeeded_dag_run_state = dag_run_state.mark_succeeded(&node_id)?;
            if succeeded_dag_run_state.all_succeeded() {
                self.execution_queue_repository.complete(&queued_task)?;
                self.run_state_repository.save(
                    &started_run_state
                        .succeed()
                        .with_dag_run_state(succeeded_dag_run_state),
                )?;
                return Ok(RunNextOutcome::Succeeded(queued_task.task_id().to_string()));
            }
            self.execution_queue_repository.requeue(&queued_task)?;
            self.run_state_repository.save(
                &started_run_state
                    .queue()
                    .with_dag_run_state(succeeded_dag_run_state),
            )?;
            return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
        }
        if started_run_state.can_retry() {
            let pending_dag_run_state =
                dag_run_state.mark_pending(&node_id, "execution failed".to_string())?;
            self.execution_queue_repository.requeue(&queued_task)?;
            self.run_state_repository.save(
                &started_run_state
                    .queue()
                    .with_dag_run_state(pending_dag_run_state),
            )?;
            return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
        }
        let failed_dag_run_state =
            dag_run_state.mark_failed(&node_id, "execution failed".to_string())?;
        self.execution_queue_repository.complete(&queued_task)?;
        self.run_state_repository.save(
            &started_run_state
                .fail()
                .with_dag_run_state(failed_dag_run_state),
        )?;
        Ok(RunNextOutcome::Failed(queued_task.task_id().to_string()))
    }

    fn resolve_linear_execution(
        &self,
        queued_task: QueuedTask,
        started_run_state: RunState,
        execution: Result<crate::TaskExecution, String>,
    ) -> Result<RunNextOutcome, String> {
        let execution = execution?;
        if execution.was_successful() {
            self.execution_queue_repository.complete(&queued_task)?;
            self.run_state_repository
                .save(&started_run_state.succeed())?;
            return Ok(RunNextOutcome::Succeeded(queued_task.task_id().to_string()));
        }
        if started_run_state.can_retry() {
            self.execution_queue_repository.requeue(&queued_task)?;
            self.run_state_repository.save(&started_run_state.queue())?;
            return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
        }
        self.execution_queue_repository.complete(&queued_task)?;
        self.run_state_repository.save(&started_run_state.fail())?;
        Ok(RunNextOutcome::Failed(queued_task.task_id().to_string()))
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
            let plan = self.plan_task(run_state, task_spec)?;
            let planned = match plan {
                Plan::Waiting => continue,
                Plan::Ready(value) => value,
            };
            let blocked = self
                .lease_repository
                .blocked_resources(&planned.placement)?;
            if !blocked.is_empty() {
                continue;
            }
            let claimed = self.execution_queue_repository.claim(&queued_task)?;
            return Ok(Selection::Selected(SelectedTask {
                queued_task: claimed,
                run_state: planned.run_state,
                task_spec: planned.task_spec,
                active_node_id: planned.active_node_id,
                placement: planned.placement,
            }));
        }
        Ok(Selection::NoneAdmissible)
    }

    fn plan_task(&self, run_state: RunState, task_spec: TaskSpec) -> Result<Plan, String> {
        if !task_spec.has_dag() {
            let placement = run_state.placement().clone();
            return Ok(Plan::Ready(ReadyTask {
                run_state,
                task_spec,
                active_node_id: None,
                placement,
            }));
        }
        let run_state = self.ensure_dag_run_state(run_state, &task_spec)?;
        let dag_run_state = run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing".to_string())?;
        if dag_run_state.all_succeeded() {
            return Ok(Plan::Waiting);
        }
        let node_id = match task_spec.first_ready_node_id(&dag_run_state) {
            Some(value) => value,
            None => return Ok(Plan::Waiting),
        };
        let placement = run_state.placement().clone();
        Ok(Plan::Ready(ReadyTask {
            run_state,
            task_spec,
            active_node_id: Some(node_id),
            placement,
        }))
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

enum Selection {
    NoneQueued,
    NoneAdmissible,
    Selected(SelectedTask),
}

enum Plan {
    Waiting,
    Ready(ReadyTask),
}

struct ReadyTask {
    run_state: RunState,
    task_spec: TaskSpec,
    active_node_id: Option<ActiveNodeId>,
    placement: Placement,
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use rack_ai_domain::ActiveNodeId;
    use rack_ai_domain::AttemptLimit;
    use rack_ai_domain::Placement;
    use rack_ai_domain::RunState;
    use rack_ai_domain::RunStateDraft;
    use rack_ai_domain::RunStatus;
    use rack_ai_domain::TaskId;
    use rack_ai_domain::TimeoutSeconds;

    use super::RunNextOutcome;
    use super::RunNextTask;
    use super::RunNextTaskDependencies;
    use crate::ExecutionQueueRepository;
    use crate::LeaseRepository;
    use crate::QueuedTask;
    use crate::RunStateRepository;
    use crate::TaskExecution;
    use crate::TaskExecutionRequest;
    use crate::TaskExecutor;
    use crate::TaskSpec;
    use crate::TaskSpecRepository;

    #[test]
    fn returns_no_queued_tasks_when_queue_is_empty() {
        let task_spec_repository = IdleTaskSpecRepository;
        let lease_repository = OpenLeaseRepository;
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &EmptyQueueRepository,
            lease_repository: &lease_repository,
            run_state_repository: &IdleRunStateRepository,
            task_executor: &SuccessfulExecutor,
            task_spec_repository: &task_spec_repository,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::NoQueuedTasks));
    }

    #[test]
    fn returns_no_admissible_tasks_when_resources_are_busy() {
        let task_spec_repository = MemoryTaskSpecRepository::linear();
        let lease_repository = BusyLeaseRepository;
        let run_states = RefCell::new(vec![sample_run_state("task-busy")]);
        let queue = SingleTaskQueueRepository::new("task-busy");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
            lease_repository: &lease_repository,
            run_state_repository: &run_state_repository,
            task_executor: &SuccessfulExecutor,
            task_spec_repository: &task_spec_repository,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::NoAdmissibleTasks));
    }

    #[test]
    fn marks_successful_linear_task_succeeded() {
        let task_spec_repository = MemoryTaskSpecRepository::linear();
        let lease_repository = TrackingLeaseRepository::new();
        let run_states = RefCell::new(vec![sample_run_state("task-a")]);
        let queue = SingleTaskQueueRepository::new("task-a");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
            lease_repository: &lease_repository,
            run_state_repository: &run_state_repository,
            task_executor: &SuccessfulExecutor,
            task_spec_repository: &task_spec_repository,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::Succeeded(_)));
        assert_eq!(
            run_states.borrow().last().unwrap().status(),
            &RunStatus::Succeeded
        );
        assert_eq!(*lease_repository.acquired.borrow(), 1);
        assert_eq!(*lease_repository.released.borrow(), 1);
    }

    #[test]
    fn requeues_failed_linear_task_when_attempts_remain() {
        let task_spec_repository = MemoryTaskSpecRepository::linear();
        let lease_repository = TrackingLeaseRepository::new();
        let run_states = RefCell::new(vec![sample_run_state("task-b")]);
        let queue = SingleTaskQueueRepository::new("task-b");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
            lease_repository: &lease_repository,
            run_state_repository: &run_state_repository,
            task_executor: &FailingExecutor,
            task_spec_repository: &task_spec_repository,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::Requeued(_)));
        assert_eq!(
            run_states.borrow().last().unwrap().status(),
            &RunStatus::Queued
        );
    }

    #[test]
    fn advances_dag_task_by_requeueing_after_successful_node() {
        let task_spec_repository = MemoryTaskSpecRepository::dag();
        let lease_repository = TrackingLeaseRepository::new();
        let run_states = RefCell::new(vec![sample_dag_run_state("task-c")]);
        let queue = SingleTaskQueueRepository::new("task-c");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
            lease_repository: &lease_repository,
            run_state_repository: &run_state_repository,
            task_executor: &SuccessfulExecutor,
            task_spec_repository: &task_spec_repository,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::Requeued(_)));
        let state = run_states.borrow().last().unwrap().clone();
        let dag = state.dag_run_state().unwrap();
        let plan = dag
            .node_state(&ActiveNodeId::new("plan".to_string()).unwrap())
            .unwrap();
        assert_eq!(plan.status(), &rack_ai_domain::DagNodeStatus::Succeeded);
    }

    #[test]
    fn fails_dag_task_after_exhausting_attempts() {
        let task_spec_repository = MemoryTaskSpecRepository::dag();
        let lease_repository = TrackingLeaseRepository::new();
        let run_states = RefCell::new(vec![sample_single_attempt_dag_run_state("task-d")]);
        let queue = SingleTaskQueueRepository::new("task-d");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
            lease_repository: &lease_repository,
            run_state_repository: &run_state_repository,
            task_executor: &FailingExecutor,
            task_spec_repository: &task_spec_repository,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::Failed(_)));
        assert_eq!(
            run_states.borrow().last().unwrap().status(),
            &RunStatus::Failed
        );
    }

    struct EmptyQueueRepository;

    impl ExecutionQueueRepository for EmptyQueueRepository {
        fn list(&self) -> Result<Vec<QueuedTask>, String> {
            Ok(vec![])
        }
        fn claim(&self, _task: &QueuedTask) -> Result<QueuedTask, String> {
            Err("not used".to_string())
        }
        fn complete(&self, _task: &QueuedTask) -> Result<(), String> {
            Ok(())
        }
        fn requeue(&self, _task: &QueuedTask) -> Result<(), String> {
            Ok(())
        }
    }

    struct SingleTaskQueueRepository {
        task_id: String,
        claimed: RefCell<bool>,
    }

    impl SingleTaskQueueRepository {
        fn new(task_id: &str) -> Self {
            Self {
                task_id: task_id.to_string(),
                claimed: RefCell::new(false),
            }
        }
    }

    impl ExecutionQueueRepository for SingleTaskQueueRepository {
        fn list(&self) -> Result<Vec<QueuedTask>, String> {
            if *self.claimed.borrow() {
                return Ok(vec![]);
            }
            Ok(vec![QueuedTask::new(
                self.task_id.clone(),
                "/tmp/spec.json".to_string(),
            )])
        }

        fn claim(&self, task: &QueuedTask) -> Result<QueuedTask, String> {
            *self.claimed.borrow_mut() = true;
            Ok(QueuedTask::new(
                task.task_id().to_string(),
                "/tmp/running.json".to_string(),
            ))
        }

        fn complete(&self, _task: &QueuedTask) -> Result<(), String> {
            Ok(())
        }
        fn requeue(&self, _task: &QueuedTask) -> Result<(), String> {
            Ok(())
        }
    }

    struct MemoryRunStateRepository<'a> {
        run_states: &'a RefCell<Vec<RunState>>,
    }

    impl RunStateRepository for MemoryRunStateRepository<'_> {
        fn save(&self, run_state: &RunState) -> Result<(), String> {
            self.run_states.borrow_mut().push(run_state.clone());
            Ok(())
        }

        fn find(&self, task_id: &TaskId) -> Result<Option<RunState>, String> {
            Ok(self
                .run_states
                .borrow()
                .iter()
                .find(|item| item.task_id() == task_id)
                .cloned())
        }

        fn list(&self) -> Result<Vec<RunState>, String> {
            Ok(self.run_states.borrow().clone())
        }
    }

    struct IdleRunStateRepository;

    impl RunStateRepository for IdleRunStateRepository {
        fn save(&self, _run_state: &RunState) -> Result<(), String> {
            Ok(())
        }
        fn find(&self, _task_id: &TaskId) -> Result<Option<RunState>, String> {
            Ok(None)
        }
        fn list(&self) -> Result<Vec<RunState>, String> {
            Ok(vec![])
        }
    }

    struct MemoryTaskSpecRepository {
        task_spec: TaskSpec,
    }

    impl MemoryTaskSpecRepository {
        fn linear() -> Self {
            Self { task_spec: serde_json::from_value(serde_json::json!({
                "task_id": "task",
                "placement": {"worker_ids": ["worker"], "resource_ids": ["gpu"], "model_ids": [], "backends": []}
            })).unwrap() }
        }

        fn dag() -> Self {
            Self { task_spec: serde_json::from_value(serde_json::json!({
                "task_id": "task",
                "placement": {"worker_ids": ["planner", "coder"], "resource_ids": ["gpu"], "model_ids": [], "backends": []},
                "dag": {
                    "nodes": [
                        {"id": "plan", "worker": "planner", "cwd": "/tmp/project", "prompt": "Plan"},
                        {"id": "code", "worker": "coder", "cwd": "/tmp/project", "prompt": "Code", "depends_on": ["plan"]}
                    ]
                }
            })).unwrap() }
        }
    }

    impl TaskSpecRepository for MemoryTaskSpecRepository {
        fn save(&self, _task_id: &str, _spec_json: &str) -> Result<(), String> {
            Ok(())
        }
        fn load(&self, _task: &QueuedTask) -> Result<TaskSpec, String> {
            Ok(self.task_spec.clone())
        }
    }

    struct IdleTaskSpecRepository;

    impl TaskSpecRepository for IdleTaskSpecRepository {
        fn save(&self, _task_id: &str, _spec_json: &str) -> Result<(), String> {
            Ok(())
        }
        fn load(&self, _task: &QueuedTask) -> Result<TaskSpec, String> {
            Err("no task spec".to_string())
        }
    }

    struct SuccessfulExecutor;

    impl TaskExecutor for SuccessfulExecutor {
        fn execute(&self, _request: &TaskExecutionRequest) -> Result<TaskExecution, String> {
            Ok(TaskExecution::success())
        }
    }

    struct FailingExecutor;

    impl TaskExecutor for FailingExecutor {
        fn execute(&self, _request: &TaskExecutionRequest) -> Result<TaskExecution, String> {
            Ok(TaskExecution::failure())
        }
    }

    struct OpenLeaseRepository;

    impl LeaseRepository for OpenLeaseRepository {
        fn blocked_resources(&self, _placement: &Placement) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        fn acquire(&self, _task_id: &TaskId, _placement: &Placement) -> Result<(), String> {
            Ok(())
        }
        fn release(&self, _placement: &Placement) -> Result<(), String> {
            Ok(())
        }
    }

    struct BusyLeaseRepository;

    impl LeaseRepository for BusyLeaseRepository {
        fn blocked_resources(&self, _placement: &Placement) -> Result<Vec<String>, String> {
            Ok(vec!["gpu".to_string()])
        }
        fn acquire(&self, _task_id: &TaskId, _placement: &Placement) -> Result<(), String> {
            Ok(())
        }
        fn release(&self, _placement: &Placement) -> Result<(), String> {
            Ok(())
        }
    }

    struct TrackingLeaseRepository {
        acquired: RefCell<u32>,
        released: RefCell<u32>,
    }

    impl TrackingLeaseRepository {
        fn new() -> Self {
            Self {
                acquired: RefCell::new(0),
                released: RefCell::new(0),
            }
        }
    }

    impl LeaseRepository for TrackingLeaseRepository {
        fn blocked_resources(&self, _placement: &Placement) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        fn acquire(&self, _task_id: &TaskId, _placement: &Placement) -> Result<(), String> {
            *self.acquired.borrow_mut() += 1;
            Ok(())
        }
        fn release(&self, _placement: &Placement) -> Result<(), String> {
            *self.released.borrow_mut() += 1;
            Ok(())
        }
    }

    fn sample_run_state(task_id: &str) -> RunState {
        RunState::queued(RunStateDraft {
            task_id: TaskId::new(task_id.to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(2).unwrap(),
            timeout_seconds: TimeoutSeconds::new(60).unwrap(),
            placement: Placement::new(vec!["worker".to_string()], vec!["gpu".to_string()]),
        })
    }

    fn sample_dag_run_state(task_id: &str) -> RunState {
        RunState::queued(RunStateDraft {
            task_id: TaskId::new(task_id.to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(2).unwrap(),
            timeout_seconds: TimeoutSeconds::new(60).unwrap(),
            placement: Placement::new(vec!["planner".to_string()], vec!["gpu".to_string()]),
        })
    }

    fn sample_single_attempt_dag_run_state(task_id: &str) -> RunState {
        RunState::queued(RunStateDraft {
            task_id: TaskId::new(task_id.to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(1).unwrap(),
            timeout_seconds: TimeoutSeconds::new(60).unwrap(),
            placement: Placement::new(vec!["planner".to_string()], vec!["gpu".to_string()]),
        })
    }
}
