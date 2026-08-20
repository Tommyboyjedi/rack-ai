use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LeaseState {
    resource_id: String,
    task_id: Option<String>,
    worker_ids: Vec<String>,
    model_ids: Vec<String>,
    acquired_at: Option<String>,
    lease_path: String,
}

impl LeaseState {
    pub fn new(
        resource_id: String,
        task_id: Option<String>,
        worker_ids: Vec<String>,
        model_ids: Vec<String>,
        acquired_at: Option<String>,
        lease_path: String,
    ) -> Self {
        Self {
            resource_id,
            task_id,
            worker_ids,
            model_ids,
            acquired_at,
            lease_path,
        }
    }

    pub fn resource_id(&self) -> &str {
        self.resource_id.as_str()
    }

    pub fn task_id(&self) -> Option<&String> {
        self.task_id.as_ref()
    }
}
