use crate::backend::GateStatus;
use reqwest::blocking::RequestBuilder;
use std::io::Read;
pub enum GateObservation {
    Pending,
    Ready(GateStatus),
}
pub struct GateProbe {
    pub request: RequestBuilder,
}
impl GateProbe {
    pub fn observe(self) -> Result<GateObservation, String> {
        let response = match self.request.send() {
            Ok(r) => r,
            Err(e) if e.is_connect() || e.is_timeout() => return Ok(GateObservation::Pending),
            Err(_) => return Err("gate transport failed".into()),
        };
        if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            return Ok(GateObservation::Pending);
        }
        if !response.status().is_success() {
            return Err(format!("gate identity HTTP {}", response.status().as_u16()));
        }
        const MAX_GATE_BYTES: u64 = 4096;
        let mut data = Vec::new();
        response
            .take(MAX_GATE_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|_| "gate read failed")?;
        if data.len() as u64 > MAX_GATE_BYTES {
            return Err("gate identity oversized".into());
        }
        serde_json::from_slice(&data)
            .map(GateObservation::Ready)
            .map_err(|_| "invalid gate identity".into())
    }
}
