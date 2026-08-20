use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunMetadata {
    queue_path: Option<String>,
    result_path: Option<String>,
    last_error: Option<String>,
    submitted_at: Option<String>,
    source_spec: Option<String>,
    admission_state: Option<String>,
    #[serde(default)]
    waiting_on_resources: Vec<String>,
    #[serde(default)]
    lease_paths: BTreeMap<String, String>,
    started_at: Option<String>,
    finished_at: Option<String>,
}

impl RunMetadata {
    pub fn submitted(
        mut self,
        submitted_at: String,
        source_spec: String,
        queue_path: String,
    ) -> Self {
        self.submitted_at = Some(submitted_at);
        self.source_spec = Some(source_spec);
        self.queue_path = Some(queue_path);
        self.admission_state = Some("queued".to_string());
        self
    }

    pub fn ready(mut self, queue_path: String) -> Self {
        self.queue_path = Some(queue_path);
        self.admission_state = Some("ready".to_string());
        self.waiting_on_resources = vec![];
        self.lease_paths = BTreeMap::new();
        self
    }

    pub fn waiting_for_resources(
        mut self,
        queue_path: String,
        waiting_on_resources: Vec<String>,
    ) -> Self {
        self.queue_path = Some(queue_path);
        self.admission_state = Some("waiting_for_resources".to_string());
        self.waiting_on_resources = waiting_on_resources;
        self.lease_paths = BTreeMap::new();
        self
    }

    pub fn running(
        mut self,
        started_at: String,
        queue_path: String,
        lease_paths: BTreeMap<String, String>,
    ) -> Self {
        self.started_at = Some(started_at);
        self.queue_path = Some(queue_path);
        self.admission_state = Some("running".to_string());
        self.waiting_on_resources = vec![];
        self.lease_paths = lease_paths;
        self.last_error = None;
        self
    }

    pub fn queued(
        mut self,
        queue_path: String,
        finished_at: String,
        result_path: Option<String>,
        last_error: Option<String>,
    ) -> Self {
        self.queue_path = Some(queue_path);
        self.finished_at = Some(finished_at);
        self.result_path = result_path;
        self.last_error = last_error;
        self.admission_state = Some("queued".to_string());
        self.lease_paths = BTreeMap::new();
        self.waiting_on_resources = vec![];
        self
    }

    pub fn completed(mut self, finished_at: String, result_path: Option<String>) -> Self {
        self.finished_at = Some(finished_at);
        self.result_path = result_path;
        self.last_error = None;
        self.admission_state = Some("completed".to_string());
        self.queue_path = None;
        self.lease_paths = BTreeMap::new();
        self.waiting_on_resources = vec![];
        self
    }

    pub fn failed(
        mut self,
        finished_at: String,
        result_path: Option<String>,
        last_error: String,
    ) -> Self {
        self.finished_at = Some(finished_at);
        self.result_path = result_path;
        self.last_error = Some(last_error);
        self.admission_state = Some("failed".to_string());
        self.queue_path = None;
        self.lease_paths = BTreeMap::new();
        self.waiting_on_resources = vec![];
        self
    }

    pub fn queue_path(&self) -> Option<&String> {
        self.queue_path.as_ref()
    }
    pub fn result_path(&self) -> Option<&String> {
        self.result_path.as_ref()
    }
    pub fn last_error(&self) -> Option<&String> {
        self.last_error.as_ref()
    }
    pub fn submitted_at(&self) -> Option<&String> {
        self.submitted_at.as_ref()
    }
    pub fn source_spec(&self) -> Option<&String> {
        self.source_spec.as_ref()
    }
    pub fn admission_state(&self) -> Option<&String> {
        self.admission_state.as_ref()
    }
    pub fn waiting_on_resources(&self) -> &[String] {
        self.waiting_on_resources.as_slice()
    }
    pub fn lease_paths(&self) -> &BTreeMap<String, String> {
        &self.lease_paths
    }
    pub fn started_at(&self) -> Option<&String> {
        self.started_at.as_ref()
    }
    pub fn finished_at(&self) -> Option<&String> {
        self.finished_at.as_ref()
    }
}
