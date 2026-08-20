use crate::WorkerBinding;

pub trait WorkerCatalog {
    fn resolve(&self, worker_id: &str) -> Result<WorkerBinding, String>;
}
