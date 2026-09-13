use super::*;
use crate::{LeaseHandle, WorkerBinding};
use rack_ai_domain::{AttemptLimit, RunStateDraft, TimeoutSeconds};
use std::cell::{Cell, RefCell};
struct Fixture {
    state: RefCell<RunState>,
    queued: Cell<bool>,
    claimed: Cell<bool>,
    held: Cell<bool>,
    calls: Cell<usize>,
    saves: Cell<usize>,
    clocks: Cell<usize>,
    fail_acquire: bool,
    fail_save: usize,
    fail_clock: usize,
    executor_error: bool,
}
impl Fixture {
    fn new() -> Self {
        Self {
            state: RefCell::new(RunState::queued(RunStateDraft {
                task_id: TaskId::new("fixture".into()).unwrap(),
                placement: Placement::new(vec![], vec!["gpu-fixture".into()]),
                attempt_limit: AttemptLimit::new(2).unwrap(),
                timeout_seconds: TimeoutSeconds::new(10).unwrap(),
            })),
            queued: Cell::new(true),
            claimed: Cell::new(false),
            held: Cell::new(false),
            calls: Cell::new(0),
            saves: Cell::new(0),
            clocks: Cell::new(0),
            fail_acquire: false,
            fail_save: 0,
            fail_clock: 0,
            executor_error: false,
        }
    }
    fn execute(&self) -> Result<RunNextOutcome, String> {
        RunNextTask::new(RunNextTaskDependencies {
            clock: self,
            execution_queue_repository: self,
            lease_repository: self,
            run_state_repository: self,
            task_executor: self,
            task_spec_repository: self,
            worker_catalog: self,
        })
        .execute()
    }
}
impl Clock for Fixture {
    fn now_text(&self) -> Result<String, String> {
        self.clocks.set(self.clocks.get() + 1);
        if self.clocks.get() == self.fail_clock {
            Err("clock unavailable".into())
        } else {
            Ok("now".into())
        }
    }
}
impl ExecutionQueueRepository for Fixture {
    fn list(&self) -> Result<Vec<QueuedTask>, String> {
        Ok(if self.queued.get() {
            vec![QueuedTask::new("fixture".into(), "queued.json".into())]
        } else {
            vec![]
        })
    }
    fn claim(&self, _: &QueuedTask) -> Result<QueuedTask, String> {
        self.queued.set(false);
        self.claimed.set(true);
        Ok(QueuedTask::new("fixture".into(), "running.json".into()))
    }
    fn complete(&self, _: &QueuedTask) -> Result<(), String> {
        self.claimed.set(false);
        Ok(())
    }
    fn requeue(&self, _: &QueuedTask) -> Result<QueuedTask, String> {
        self.queued.set(true);
        self.claimed.set(false);
        Ok(QueuedTask::new("fixture".into(), "queued.json".into()))
    }
}
impl RunStateRepository for Fixture {
    fn save(&self, s: &RunState) -> Result<(), String> {
        self.saves.set(self.saves.get() + 1);
        if self.saves.get() == self.fail_save {
            return Err("persistence unavailable".into());
        }
        *self.state.borrow_mut() = s.clone();
        Ok(())
    }
    fn find(&self, _: &TaskId) -> Result<Option<RunState>, String> {
        Ok(Some(self.state.borrow().clone()))
    }
    fn list(&self) -> Result<Vec<RunState>, String> {
        Ok(vec![self.state.borrow().clone()])
    }
}
impl LeaseRepository for Fixture {
    fn blocked_resources(&self, _: &Placement) -> Result<Vec<String>, String> {
        Ok(vec![])
    }
    fn acquire(&self, _: &LeaseRequest<'_>) -> Result<LeaseHandle, String> {
        if self.fail_acquire {
            return Err("resource busy: race winner".into());
        }
        self.held.set(true);
        Ok(LeaseHandle {
            owner: "fixture".into(),
            generation: "generation".into(),
            paths: BTreeMap::new(),
        })
    }
    fn renew(&self, _: &LeaseHandle) -> Result<(), String> {
        Ok(())
    }
    fn release(&self, _: &LeaseHandle) -> Result<(), String> {
        self.held.set(false);
        Ok(())
    }
}
impl TaskExecutor for Fixture {
    fn execute(&self, _: &TaskExecutionRequest) -> Result<TaskExecution, String> {
        self.calls.set(self.calls.get() + 1);
        if self.executor_error {
            Err("worker unavailable".into())
        } else {
            Ok(TaskExecution::success(None))
        }
    }
}
impl TaskSpecRepository for Fixture {
    fn save(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn load(&self, _: &QueuedTask) -> Result<TaskSpec, String> {
        serde_json::from_str(r#"{"task_id":"fixture","placement":{"worker_ids":[],"resource_ids":["gpu-fixture"],"model_ids":[],"backends":[]}}"#).map_err(|e|e.to_string())
    }
}
impl WorkerCatalog for Fixture {
    fn resolve(&self, _: &str) -> Result<WorkerBinding, String> {
        Err("unused for linear fixture".into())
    }
}
#[test]
fn acquisition_race_returns_claimed_item_without_invocation() {
    let mut f = Fixture::new();
    f.fail_acquire = true;
    assert!(f.execute().is_err());
    assert!(f.queued.get());
    assert!(!f.claimed.get());
    assert_eq!(f.calls.get(), 0);
}
#[test]
fn clock_failure_after_claim_restores_queue() {
    let mut f = Fixture::new();
    f.fail_clock = 1;
    assert!(f.execute().is_err());
    assert!(f.queued.get());
    assert!(!f.held.get());
}
#[test]
fn preparation_save_failure_restores_queue_and_releases() {
    let mut f = Fixture::new();
    f.fail_save = 2;
    assert!(f.execute().is_err());
    assert!(f.queued.get());
    assert!(!f.held.get());
    assert_eq!(f.calls.get(), 0);
}
#[test]
fn completion_persistence_failure_retains_claim_and_never_reexecutes() {
    let mut f = Fixture::new();
    f.fail_save = 3;
    assert!(f.execute().is_err());
    assert!(!f.queued.get());
    assert!(f.claimed.get());
    assert!(!f.held.get());
    assert!(matches!(
        f.execute().unwrap(),
        RunNextOutcome::NoQueuedTasks
    ));
    assert_eq!(f.calls.get(), 1);
}
#[test]
fn executor_error_uses_existing_retry_policy_and_releases() {
    let mut f = Fixture::new();
    f.executor_error = true;
    assert!(matches!(f.execute().unwrap(), RunNextOutcome::Requeued(_)));
    assert!(!f.held.get());
    assert!(f.queued.get());
}
#[test]
fn retry_state_persistence_failure_does_not_publish_queue_item() {
    let mut f = Fixture::new();
    f.executor_error = true;
    f.fail_save = 3;
    assert!(f.execute().is_err());
    assert!(f.claimed.get());
    assert!(!f.queued.get());
    assert!(!f.held.get());
}
