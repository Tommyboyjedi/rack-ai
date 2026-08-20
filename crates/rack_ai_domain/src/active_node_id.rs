use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ActiveNodeId(String);

impl ActiveNodeId {
    pub fn new(value: String) -> Result<Self, String> {
        if value.trim().is_empty() {
            return Err("active node id cannot be empty".to_string());
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::ActiveNodeId;

    #[test]
    fn rejects_blank_node_id() {
        let result = ActiveNodeId::new("".to_string());
        assert_eq!(result, Err("active node id cannot be empty".to_string()));
    }

    #[test]
    fn accepts_node_id() {
        let node_id = ActiveNodeId::new("verify".to_string()).unwrap();
        assert_eq!(node_id.value(), "verify");
    }
}
