use rack_ai_domain::Placement;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerBinding {
    worker_id: String,
    resource_id: String,
    model_id: String,
    backend: String,
}

impl WorkerBinding {
    pub fn new(worker_id: String, resource_id: String, model_id: String, backend: String) -> Self {
        Self {
            worker_id,
            resource_id,
            model_id,
            backend,
        }
    }

    pub fn placement(&self) -> Placement {
        Placement::new(vec![self.worker_id.clone()], vec![self.resource_id.clone()])
            .with_models(vec![self.model_id.clone()])
            .with_backends(vec![self.backend.clone()])
    }
}

#[cfg(test)]
mod tests {
    use super::WorkerBinding;

    #[test]
    fn builds_placement_from_binding() {
        let binding = WorkerBinding::new(
            "local-coder".to_string(),
            "gpu-2060".to_string(),
            "coder-model".to_string(),
            "vllm".to_string(),
        );
        let placement = binding.placement();
        assert_eq!(placement.worker_ids(), ["local-coder".to_string()]);
        assert_eq!(placement.resource_ids(), ["gpu-2060".to_string()]);
        assert_eq!(placement.model_ids(), ["coder-model".to_string()]);
        assert_eq!(placement.backends(), ["vllm".to_string()]);
    }
}
