//! Preserve semantic whitespace and source text while extending public diagnostic redaction.
use crate::{config::Config, types::Document};
use serde_json::Value;

pub struct Redactor {
    private_values: Vec<String>,
}
impl Redactor {
    pub fn new(config: &Config, state: &Document) -> Self {
        let mut values = vec![config.authority_root.to_string_lossy().into_owned()];
        for profile in &config.profiles {
            values.push(profile.endpoint.clone());
        }
        for demand in state.data.demands.values() {
            values.push(demand.access_key.clone());
        }
        for (key, value) in std::env::vars_os() {
            let key = key.to_string_lossy();
            let value = value.to_string_lossy().into_owned();
            if sensitive(&key) && value.len() >= 4 {
                values.push(value);
            }
        }
        values.retain(|s| !s.is_empty());
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        Self {
            private_values: values,
        }
    }
    pub fn value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.text(text)),
            Value::Array(values) => Value::Array(values.iter().map(|v| self.value(v)).collect()),
            Value::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(key, value)| {
                        (
                            self.text(key),
                            if sensitive(key) {
                                Value::String("<redacted:credential>".into())
                            } else {
                                self.value(value)
                            },
                        )
                    })
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
    pub fn text(&self, text: &str) -> String {
        let mut text = text.to_string();
        for value in &self.private_values {
            text = text.replace(value, "<redacted:private>");
        }
        let mut bearer = false;
        let mut assignment = false;
        let mut output = String::new();
        for part in text.split_inclusive(char::is_whitespace) {
            let word = part.trim_end_matches(char::is_whitespace);
            let space = &part[word.len()..];
            let lower = word.to_ascii_lowercase();
            let core = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            let inline_secret = word
                .split_once('=')
                .or_else(|| word.split_once(':'))
                .is_some_and(|(key, value)| {
                    sensitive(key.trim_matches(|c: char| !c.is_alphanumeric() && c != '_'))
                        && !value.is_empty()
                });
            let redact = bearer || assignment || inline_secret;
            bearer = core.eq_ignore_ascii_case("bearer");
            assignment = sensitive(core) && (word.ends_with(':') || word.ends_with('='));
            if redact {
                output.push_str("<redacted:credential>");
            } else if private_url(&lower) {
                output.push_str("<redacted:endpoint>");
            } else {
                output.push_str(&crate::work_execution_view::redact_interaction_word(word));
            }
            output.push_str(space);
        }
        output
    }
}
fn sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "password",
        "passwd",
        "secret",
        "api_key",
        "apikey",
        "access_key",
        "access_token",
        "auth_token",
        "bearer_token",
        "capability",
        "credential",
    ]
    .iter()
    .any(|s| key.contains(s))
        || key.ends_with("_token")
        || key == "token"
}
fn private_url(word: &str) -> bool {
    word.contains("http://127.")
        || word.contains("https://127.")
        || word.contains("://localhost")
        || word.contains("://10.")
        || word.contains("://192.168.")
        || word.contains("://[::1]")
        || word.contains("/scoped/")
}
