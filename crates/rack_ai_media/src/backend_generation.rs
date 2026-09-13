use crate::systemd::Observation;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BackendGeneration {
    pub number: u64,
    pub invocation: String,
    pub pid: u32,
    pub cgroup: String,
    pub start_ticks: u64,
}
impl BackendGeneration {
    pub fn capture(number: u64, observed: &Observation) -> Result<Self, String> {
        Ok(Self {
            number,
            invocation: observed.invocation.clone(),
            pid: observed.pid,
            cgroup: observed.cgroup.clone(),
            start_ticks: start_ticks(observed.pid)?,
        })
    }
    pub fn verify(&self, observed: &Observation) -> Result<(), String> {
        if *self != Self::capture(self.number, observed)? {
            return Err("backend process generation changed".into());
        }
        Ok(())
    }
}
pub fn start_ticks(pid: u32) -> Result<u64, String> {
    present_start_ticks(pid)?.ok_or("process identity disappeared".into())
}
pub fn present_start_ticks(pid: u32) -> Result<Option<u64>, String> {
    let raw = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("process start identity unavailable".into()),
    };
    raw.rsplit_once(')')
        .and_then(|(_, tail)| tail.split_whitespace().nth(19))
        .and_then(|v| v.parse().ok())
        .map(Some)
        .ok_or("invalid process start identity".into())
}
