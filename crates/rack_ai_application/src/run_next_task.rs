use rack_ai_domain::RunState;
use rack_ai_domain::TaskId;

use crate::ExecutionQueueRepository;
use crate::QueuedTask;
use crate::RunStateRepository;
use crate::TaskExecutionRequest;
use crate::TaskExecutor;
use crate::TaskSpec;
use crate::TaskSpecRepository;

pub struct RunNextTask<'a> {
    execution_queue_repository: &'a dyn ExecutionQueueRepository,
    run_state_repository: &'a dyn RunStateRepository,
    task_executor: &'a dyn TaskExecutor,
    task_spec_repository: &'a dyn TaskSpecRepository,
}

pub struct RunNextTaskDependencies<'a> {
    pub execution_queue_repository: &'a dyn ExecutionQueueRepository,
    pub run_state_repository: &'a dyn RunStateRepository,
    pub task_executor: &'a dyn TaskExecutor,
    pub task_spec_repository: &'a dyn TaskSpecRepository,
}

pub enum RunNextOutcome {
    NoQueuedTasks,
    Succeeded(String),
    Requeued(String),
    Failed(String),
}

impl<'a> RunNextTask<'a> {
    pub fn new(dependencies: RunNextTaskDependencies<'a>) -> Self {
        Self {
            execution_queue_repository: dependencies.execution_queue_repository,
            run_state_repository: dependencies.run_state_repository,
            task_executor: dependencies.task_executor,
            task_spec_repository: dependencies.task_spec_repository,
        }
    }

    pub fn execute(&self) -> Result<RunNextOutcome, String> {
        let queued_task = match self.execution_queue_repository.take_next()? {
            Some(value) => value,
            None => return Ok(RunNextOutcome::NoQueuedTasks),
        };
        let task_id = TaskId::new(queued_task.task_id().to_string())?;
        let run_state = self.load_run_state(&task_id)?;
        let task_spec = self.task_spec_repository.load(&queued_task)?;
        if task_spec.has_dag() {
            return self.execute_dag_task(queued_task, run_state, task_spec);
        }
        self.execute_linear_task(queued_task, run_state)
    }

    fn execute_dag_task(
        &self,
        queued_task: QueuedTask,
        run_state: RunState,
        task_spec: TaskSpec,
    ) -> Result<RunNextOutcome, String> {
        let current_run_state = self.ensure_dag_run_state(run_state, &task_spec)?;
        let dag_run_state = current_run_state
            .dag_run_state()
            .cloned()
            .ok_or("dag run state missing".to_string())?;
        if dag_run_state.all_succeeded() {
            self.execution_queue_repository.complete(&queued_task)?;
            self.run_state_repository.save(
                &current_run_state
                    .succeed()
                    .with_dag_run_state(dag_run_state),
            )?;
            return Ok(RunNextOutcome::Succeeded(queued_task.task_id().to_string()));
        }
        let node_id = task_spec
            .first_ready_node_id(&dag_run_state)
            .ok_or("no ready dag node available".to_string())?;
        let started_dag_run_state = dag_run_state.mark_running(&node_id)?;
        let started_run_state = current_run_state
            .start(Some(node_id.clone()))
            .with_dag_run_state(started_dag_run_state.clone());
        self.run_state_repository.save(&started_run_state)?;
        let execution_request = TaskExecutionRequest::new(
            queued_task.task_id().to_string(),
            queued_task.spec_path().to_string(),
        )
        .with_execution_spec_json(task_spec.build_execution_spec_json(&node_id)?);
        let execution = self.task_executor.execute(&execution_request)?;
        if execution.was_successful() {
            let succeeded_dag_run_state = started_dag_run_state.mark_succeeded(&node_id)?;
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
                started_dag_run_state.mark_pending(&node_id, "execution failed".to_string())?;
            self.execution_queue_repository.requeue(&queued_task)?;
            self.run_state_repository.save(
                &started_run_state
                    .queue()
                    .with_dag_run_state(pending_dag_run_state),
            )?;
            return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
        }
        let failed_dag_run_state =
            started_dag_run_state.mark_failed(&node_id, "execution failed".to_string())?;
        self.execution_queue_repository.complete(&queued_task)?;
        self.run_state_repository.save(
            &started_run_state
                .fail()
                .with_dag_run_state(failed_dag_run_state),
        )?;
        Ok(RunNextOutcome::Failed(queued_task.task_id().to_string()))
    }

    fn execute_linear_task(
        &self,
        queued_task: QueuedTask,
        run_state: RunState,
    ) -> Result<RunNextOutcome, String> {
        let started_run_state = run_state.start(None);
        self.run_state_repository.save(&started_run_state)?;
        let execution_request = TaskExecutionRequest::new(
            queued_task.task_id().to_string(),
            queued_task.spec_path().to_string(),
        );
        let execution = self.task_executor.execute(&execution_request)?;
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
    use crate::QueuedTask;
    use crate::RunStateRepository;
    use crate::TaskExecution;
    use crate::TaskExecutionRequest;
    use crate::TaskExecutor;
    use crate::TaskSpec;
    use crate::TaskSpecRepository;

    #[test]
    fn returns_no_queued_tasks_when_queue_is_empty() {
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &EmptyQueueRepository,
            run_state_repository: &IdleRunStateRepository,
            task_executor: &SuccessfulExecutor,
            task_spec_repository: &IdleTaskSpecRepository,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::NoQueuedTasks));
    }

    #[test]
    fn marks_successful_linear_task_succeeded() {
        let run_states = RefCell::new(vec![sample_run_state("task-a")]);
        let queue = SingleTaskQueueRepository::new("task-a");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let task_spec_repository = MemoryTaskSpecRepository::linear();
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
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
    }

    #[test]
    fn requeues_failed_linear_task_when_attempts_remain() {
        let run_states = RefCell::new(vec![sample_run_state("task-b")]);
        let queue = SingleTaskQueueRepository::new("task-b");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let task_spec_repository = MemoryTaskSpecRepository::linear();
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
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
        let run_states = RefCell::new(vec![sample_run_state("task-c")]);
        let queue = SingleTaskQueueRepository::new("task-c");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let task_spec_repository = MemoryTaskSpecRepository::dag();
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
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
        let run_states = RefCell::new(vec![sample_single_attempt_run_state("task-d")]);
        let queue = SingleTaskQueueRepository::new("task-d");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let task_spec_repository = MemoryTaskSpecRepository::dag();
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
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
        fn take_next(&self) -> Result<Option<QueuedTask>, String> {
            Ok(None)
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
        taken: RefCell<bool>,
    }

    impl SingleTaskQueueRepository {
        fn new(task_id: &str) -> Self {
            Self {
                task_id: task_id.to_string(),
                taken: RefCell::new(false),
            }
        }
    }

    impl ExecutionQueueRepository for SingleTaskQueueRepository {
        fn take_next(&self) -> Result<Option<QueuedTask>, String> {
            if *self.taken.borrow() {
                return Ok(None);
            }
            *self.taken.borrow_mut() = true;
            Ok(Some(QueuedTask::new(
                self.task_id.clone(),
                "/tmp/spec.json".to_string(),
            )))
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

    fn sample_run_state(task_id: &str) -> RunState {
        RunState::queued(RunStateDraft {
            task_id: TaskId::new(task_id.to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(2).unwrap(),
            timeout_seconds: TimeoutSeconds::new(60).unwrap(),
            placement: Placement::new(vec!["worker".to_string()], vec!["gpu".to_string()]),
        })
    }

    fn sample_single_attempt_run_state(task_id: &str) -> RunState {
        RunState::queued(RunStateDraft {
            task_id: TaskId::new(task_id.to_string()).unwrap(),
            attempt_limit: AttemptLimit::new(1).unwrap(),
            timeout_seconds: TimeoutSeconds::new(60).unwrap(),
            placement: Placement::new(vec!["worker".to_string()], vec!["gpu".to_string()]),
        })
    }
}
