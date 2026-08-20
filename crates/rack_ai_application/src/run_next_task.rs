use rack_ai_domain::RunState;
use rack_ai_domain::TaskId;

use crate::ExecutionQueueRepository;
use crate::RunStateRepository;
use crate::TaskExecutor;

pub struct RunNextTask<'a> {
    execution_queue_repository: &'a dyn ExecutionQueueRepository,
    run_state_repository: &'a dyn RunStateRepository,
    task_executor: &'a dyn TaskExecutor,
}

pub struct RunNextTaskDependencies<'a> {
    pub execution_queue_repository: &'a dyn ExecutionQueueRepository,
    pub run_state_repository: &'a dyn RunStateRepository,
    pub task_executor: &'a dyn TaskExecutor,
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
        }
    }

    pub fn execute(&self) -> Result<RunNextOutcome, String> {
        let queued_task = match self.execution_queue_repository.take_next()? {
            Some(value) => value,
            None => return Ok(RunNextOutcome::NoQueuedTasks),
        };
        let task_id = TaskId::new(queued_task.task_id().to_string())?;
        let run_state = self.load_run_state(&task_id)?;
        let running = run_state.start(None);
        self.run_state_repository.save(&running)?;
        let execution = self.task_executor.execute(&queued_task)?;
        if execution.was_successful() {
            self.execution_queue_repository.complete(&queued_task)?;
            self.run_state_repository.save(&running.succeed())?;
            return Ok(RunNextOutcome::Succeeded(queued_task.task_id().to_string()));
        }
        if running.can_retry() {
            self.execution_queue_repository.requeue(&queued_task)?;
            self.run_state_repository.save(&running.queue())?;
            return Ok(RunNextOutcome::Requeued(queued_task.task_id().to_string()));
        }
        self.execution_queue_repository.complete(&queued_task)?;
        self.run_state_repository.save(&running.fail())?;
        Ok(RunNextOutcome::Failed(queued_task.task_id().to_string()))
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

    use rack_ai_domain::AttemptLimit;
    use rack_ai_domain::Placement;
    use rack_ai_domain::RunState;
    use rack_ai_domain::RunStateDraft;
    use rack_ai_domain::TaskId;
    use rack_ai_domain::TimeoutSeconds;

    use super::RunNextOutcome;
    use super::RunNextTask;
    use super::RunNextTaskDependencies;
    use crate::ExecutionQueueRepository;
    use crate::QueuedTask;
    use crate::RunStateRepository;
    use crate::TaskExecution;
    use crate::TaskExecutor;

    #[test]
    fn returns_no_queued_tasks_when_queue_is_empty() {
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &EmptyQueueRepository,
            run_state_repository: &IdleRunStateRepository,
            task_executor: &SuccessfulExecutor,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::NoQueuedTasks));
    }

    #[test]
    fn marks_successful_task_succeeded() {
        let run_states = RefCell::new(vec![sample_run_state("task-a")]);
        let queue = SingleTaskQueueRepository::new("task-a");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
            run_state_repository: &run_state_repository,
            task_executor: &SuccessfulExecutor,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::Succeeded(_)));
        assert_eq!(
            run_states.borrow().last().unwrap().status(),
            &rack_ai_domain::RunStatus::Succeeded
        );
    }

    #[test]
    fn requeues_failed_task_when_attempts_remain() {
        let run_states = RefCell::new(vec![sample_run_state("task-b")]);
        let queue = SingleTaskQueueRepository::new("task-b");
        let run_state_repository = MemoryRunStateRepository {
            run_states: &run_states,
        };
        let service = RunNextTask::new(RunNextTaskDependencies {
            execution_queue_repository: &queue,
            run_state_repository: &run_state_repository,
            task_executor: &FailingExecutor,
        });
        let outcome = service.execute().unwrap();
        assert!(matches!(outcome, RunNextOutcome::Requeued(_)));
        assert_eq!(
            run_states.borrow().last().unwrap().status(),
            &rack_ai_domain::RunStatus::Queued
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

    struct SuccessfulExecutor;

    impl TaskExecutor for SuccessfulExecutor {
        fn execute(&self, _task: &QueuedTask) -> Result<TaskExecution, String> {
            Ok(TaskExecution::success())
        }
    }

    struct FailingExecutor;

    impl TaskExecutor for FailingExecutor {
        fn execute(&self, _task: &QueuedTask) -> Result<TaskExecution, String> {
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
}
