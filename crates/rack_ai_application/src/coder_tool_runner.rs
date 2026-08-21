use serde_json::Value;

pub trait CoderToolRunner {
    fn run(&self, name: &str, arguments: &Value) -> Result<String, String>;
}
