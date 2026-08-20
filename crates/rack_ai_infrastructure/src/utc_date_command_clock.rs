use std::process::Command;

use rack_ai_application::Clock;

pub struct UtcDateCommandClock;

impl Clock for UtcDateCommandClock {
    fn now_text(&self) -> Result<String, String> {
        let output = Command::new("date")
            .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err("date command failed".to_string());
        }
        let text = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
        Ok(text.trim().to_string())
    }
}
