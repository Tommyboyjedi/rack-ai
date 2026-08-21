#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementChangeResult {
    output: String,
}

impl ImplementChangeResult {
    pub fn new(output: String) -> Self {
        Self { output }
    }

    pub fn output(&self) -> &str {
        self.output.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::ImplementChangeResult;

    #[test]
    fn stores_model_output() {
        let result = ImplementChangeResult::new("COMPLETE".to_string());
        assert_eq!(result.output(), "COMPLETE");
    }
}
