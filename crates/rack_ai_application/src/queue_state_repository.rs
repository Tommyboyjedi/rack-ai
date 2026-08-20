pub trait QueueStateRepository {
    fn queued_entries(&self) -> Result<Vec<String>, String>;
    fn running_entries(&self) -> Result<Vec<String>, String>;
}
