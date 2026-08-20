pub trait TaskSpecRepository {
    fn save(&self, task_id: &str, spec_json: &str) -> Result<(), String>;
}
