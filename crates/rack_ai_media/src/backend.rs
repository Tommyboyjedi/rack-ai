use crate::{
    config::Config,
    types::{Job, Mode},
};
use reqwest::blocking::Client;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};
#[derive(Clone)]
pub struct Backend {
    client: Client,
    base: String,
    secret: String,
}
#[derive(Deserialize)]
pub struct GateStatus {
    pub activation: String,
    pub invocation: String,
    pub pid: u32,
    pub mode: Mode,
    pub protocol: String,
}
#[derive(Deserialize)]
pub struct Queue {
    pub queue_running: Vec<Value>,
    pub queue_pending: Vec<Value>,
}
impl Queue {
    pub fn empty(&self) -> bool {
        self.queue_running.is_empty() && self.queue_pending.is_empty()
    }
    pub fn contains(&self, id: &str) -> bool {
        self.queue_running
            .iter()
            .chain(&self.queue_pending)
            .any(|v| v.get(1).and_then(Value::as_str) == Some(id))
    }
}
impl Backend {
    pub fn new(config: &Config) -> Result<Self, String> {
        let secret =
            std::fs::read_to_string(&config.control_secret_file).map_err(|e| e.to_string())?;
        if secret.trim().len() < 32 {
            return Err("control secret too short".into());
        }
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .map_err(|e| e.to_string())?,
            base: config.backend.trim_end_matches('/').into(),
            secret: secret.trim().into(),
        })
    }
    pub fn secret(&self) -> &str {
        &self.secret
    }
    pub fn read<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        self.decode(
            self.client
                .get(format!("{}{path}", self.base))
                .header("X-Rack-Control", &self.secret),
        )
    }
    pub fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T, String> {
        self.decode(
            self.client
                .post(format!("{}{path}", self.base))
                .header("X-Rack-Control", &self.secret)
                .header("X-Rack-Access", "managed")
                .json(body),
        )
    }
    fn decode<T: DeserializeOwned>(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<T, String> {
        let response = request.send().map_err(|_| "backend transport uncertain")?;
        if !response.status().is_success() {
            return Err(format!("backend HTTP {}", response.status().as_u16()));
        }
        const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
        let mut data = Vec::new();
        response
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut data)
            .map_err(|_| "backend read uncertain")?;
        if data.len() as u64 > MAX_RESPONSE {
            return Err("backend response oversized".into());
        }
        serde_json::from_slice(&data).map_err(|_| "invalid backend response".into())
    }
    pub fn probe_gate(&self) -> Result<crate::gate_probe::GateObservation, String> {
        crate::gate_probe::GateProbe {
            request: self
                .client
                .get(format!("{}/rack-gate/status", self.base))
                .header("X-Rack-Control", &self.secret),
        }
        .observe()
    }
    pub fn gate(&self) -> Result<GateStatus, String> {
        self.read("/rack-gate/status")
    }
    pub fn barrier(&self) -> Result<GateStatus, String> {
        self.post("/rack-gate/barrier", &json!({}))
    }
    pub fn queue(&self) -> Result<Queue, String> {
        self.read("/queue")
    }
    pub fn submit(&self, job: &Job) -> Result<(), String> {
        let reply: Value = self.post(
            "/prompt",
            &json!({"prompt_id":job.prompt_id,"prompt":job.workflow}),
        )?;
        if reply.get("prompt_id").and_then(Value::as_str) != Some(&job.prompt_id)
            || reply
                .get("node_errors")
                .and_then(Value::as_object)
                .is_none_or(|e| !e.is_empty())
        {
            return Err("submission acknowledgement incomplete".into());
        }
        Ok(())
    }
    pub fn history(&self, job: &Job) -> Result<Value, String> {
        self.read(&format!("/history/{}", job.prompt_id))
    }
    pub fn cancel(&self, job: &Job) -> Result<Value, String> {
        self.post(&format!("/api/jobs/{}/cancel", job.prompt_id), &json!({}))
    }
}
