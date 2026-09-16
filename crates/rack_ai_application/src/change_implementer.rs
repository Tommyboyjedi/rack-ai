use crate::ImplementChangeRequest;
use crate::ImplementChangeResult;

pub trait ChangeImplementer {
    fn check_execution(&self) -> Result<(), String> {
        Ok(())
    }
    fn implement(&self, request: &ImplementChangeRequest) -> Result<ImplementChangeResult, String>;
}
