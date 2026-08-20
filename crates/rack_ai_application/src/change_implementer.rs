use crate::ImplementChangeRequest;
use crate::ImplementChangeResult;

pub trait ChangeImplementer {
    fn implement(&self, request: &ImplementChangeRequest) -> Result<ImplementChangeResult, String>;
}
