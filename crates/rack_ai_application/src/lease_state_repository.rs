use crate::LeaseState;

pub trait LeaseStateRepository {
    fn list(&self) -> Result<Vec<LeaseState>, String>;
}
