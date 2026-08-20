use std::process::Child;
use std::process::Output;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub struct WallClockWait;

impl WallClockWait {
    pub fn child_output(child: Child, timeout_seconds: u32) -> Result<WaitOutcome, String> {
        let timeout = Duration::from_secs(u64::from(timeout_seconds.max(1)));
        let pid = child.id();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(child.wait_with_output());
        });
        match receiver.recv_timeout(timeout) {
            Ok(result) => result
                .map(WaitOutcome::Completed)
                .map_err(|error| error.to_string()),
            Err(_) => {
                let _ = std::process::Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .status();
                let _ = receiver.recv_timeout(Duration::from_secs(2));
                Ok(WaitOutcome::TimedOut)
            }
        }
    }
}

pub enum WaitOutcome {
    Completed(Output),
    TimedOut,
}

#[cfg(test)]
mod tests {
    use super::WaitOutcome;
    use super::WallClockWait;
    use std::process::Command;
    use std::process::Stdio;

    #[test]
    fn completes_fast_command() {
        let child = Command::new("true")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let outcome = WallClockWait::child_output(child, 5).unwrap();
        match outcome {
            WaitOutcome::Completed(output) => assert!(output.status.success()),
            WaitOutcome::TimedOut => panic!("true should not time out"),
        }
    }

    #[test]
    fn kills_hung_command() {
        let child = Command::new("sleep")
            .arg("20")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let outcome = WallClockWait::child_output(child, 1).unwrap();
        assert!(matches!(outcome, WaitOutcome::TimedOut));
    }
}
