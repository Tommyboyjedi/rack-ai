use rack_ai_domain::AcceptanceCommand;

pub trait CommandPolicy {
    fn assert_allowed(&self, command: &AcceptanceCommand) -> Result<(), String>;
}
