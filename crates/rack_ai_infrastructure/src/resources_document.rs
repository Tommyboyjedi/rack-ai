use serde::Deserialize;

use crate::ResourceRecord;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ResourcesDocument {
    pub resources: Vec<ResourceRecord>,
}
