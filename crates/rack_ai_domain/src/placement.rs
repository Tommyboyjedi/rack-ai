use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Placement {
    worker_ids: Vec<String>,
    resource_ids: Vec<String>,
    model_ids: Vec<String>,
    backends: Vec<String>,
}

impl Placement {
    pub fn new(worker_ids: Vec<String>, resource_ids: Vec<String>) -> Self {
        Self {
            worker_ids,
            resource_ids,
            model_ids: Vec::new(),
            backends: Vec::new(),
        }
    }

    pub fn with_models(mut self, model_ids: Vec<String>) -> Self {
        self.model_ids = model_ids;
        self
    }

    pub fn with_backends(mut self, backends: Vec<String>) -> Self {
        self.backends = backends;
        self
    }

    pub fn worker_ids(&self) -> &[String] {
        self.worker_ids.as_slice()
    }
    pub fn resource_ids(&self) -> &[String] {
        self.resource_ids.as_slice()
    }
    pub fn model_ids(&self) -> &[String] {
        self.model_ids.as_slice()
    }
    pub fn backends(&self) -> &[String] {
        self.backends.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::Placement;

    #[test]
    fn stores_worker_and_resource_ids() {
        let placement = Placement::new(vec!["coder".to_string()], vec!["gpu-2060".to_string()]);
        assert_eq!(placement.worker_ids(), ["coder".to_string()]);
        assert_eq!(placement.resource_ids(), ["gpu-2060".to_string()]);
    }

    #[test]
    fn supports_optional_model_and_backend_lists() {
        let placement = Placement::new(vec![], vec![])
            .with_models(vec!["model-a".to_string()])
            .with_backends(vec!["vllm".to_string()]);
        assert_eq!(placement.model_ids(), ["model-a".to_string()]);
        assert_eq!(placement.backends(), ["vllm".to_string()]);
    }
}
