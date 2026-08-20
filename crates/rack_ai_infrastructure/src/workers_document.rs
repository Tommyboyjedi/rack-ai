use serde::Deserialize;

use crate::WorkerRecord;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WorkersDocument {
    pub workers: Vec<WorkerRecord>,
}
