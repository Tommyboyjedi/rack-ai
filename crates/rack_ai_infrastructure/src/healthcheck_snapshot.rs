use serde::Serialize;

use crate::HealthcheckEntry;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthcheckSnapshot {
    pub ok: bool,
    pub checks: Vec<HealthcheckEntry>,
}
