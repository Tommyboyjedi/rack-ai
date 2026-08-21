use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionStatus {
    Retained,
}

#[cfg(test)]
mod tests {
    use super::RetentionStatus;

    #[test]
    fn v1_retention_is_explicit() {
        match RetentionStatus::Retained {
            RetentionStatus::Retained => {}
        }
    }
}
