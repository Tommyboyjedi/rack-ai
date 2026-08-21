use std::cell::RefCell;

use rack_ai_application::CoderToolRunner;
use rack_ai_application::ToolCallRecord;
use serde_json::Value;

pub struct RecordingCoderToolRunner<'a> {
    inner: &'a dyn CoderToolRunner,
    calls: RefCell<Vec<ToolCallRecord>>,
}

impl<'a> RecordingCoderToolRunner<'a> {
    pub fn new(inner: &'a dyn CoderToolRunner) -> Self {
        Self {
            inner,
            calls: RefCell::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<ToolCallRecord> {
        self.calls.borrow().clone()
    }
}

impl CoderToolRunner for RecordingCoderToolRunner<'_> {
    fn run(&self, name: &str, arguments: &Value) -> Result<String, String> {
        let result = self.inner.run(name, arguments);
        self.calls.borrow_mut().push(ToolCallRecord {
            name: name.to_string(),
            arguments: arguments.to_string(),
            result: match &result {
                Ok(value) => value.clone(),
                Err(error) => format!("error: {error}"),
            },
        });
        result
    }
}
