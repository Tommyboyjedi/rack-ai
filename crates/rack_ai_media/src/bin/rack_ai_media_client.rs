// Thin CLI transport: it cannot choose a resource root or control systemd directly.
use reqwest::blocking::Client;
use std::{env, fs, io::Read, os::unix::fs::PermissionsExt, time::Duration};
fn main() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    let endpoint = env::var("RACK_AI_ENDPOINT").map_err(|_| "set RACK_AI_ENDPOINT")?;
    let origin = reqwest::Url::parse(&endpoint).map_err(|_| "invalid endpoint")?;
    if !(origin.scheme() == "https"
        || (origin.scheme() == "http" && origin.host_str() == Some("127.0.0.1")))
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
        || !origin.username().is_empty()
        || origin.password().is_some()
    {
        return Err("use a private HTTPS origin or loopback test origin".into());
    }
    let credential =
        env::var("RACK_AI_CREDENTIAL_FILE").map_err(|_| "set RACK_AI_CREDENTIAL_FILE")?;
    if fs::metadata(&credential)
        .map_err(|_| "credential missing")?
        .permissions()
        .mode()
        & 0o077
        != 0
    {
        return Err("credential must be mode 0600".into());
    }
    let token = fs::read_to_string(credential).map_err(|_| "credential unreadable")?;
    let (method, path, body) = command(&args)?;
    let response = Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?
        .request(
            method,
            format!("{}/api/media/v1/{path}", endpoint.trim_end_matches('/')),
        )
        .bearer_auth(token.trim())
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(|_| "transport outcome uncertain; replay only the identical request")?;
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "response interrupted")?;
    if bytes.len() > 1024 * 1024 {
        return Err("oversized response".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "invalid response")?;
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
    );
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    Ok(())
}
fn command(args: &[String]) -> Result<(reqwest::Method, String, Vec<u8>), String> {
    use reqwest::Method;
    let (method, path, body) = match args.iter().map(|s| s.as_str()).collect::<Vec<_>>().as_slice() {
        ["status"] => (Method::GET, "status".into(), vec![]),
        ["profiles"] => (Method::GET, "profiles".into(), vec![]),
        ["start", key] => (Method::POST, "sessions".into(),
            serde_json::json!({"schema":"rack-ai/media/v1","idempotency_key":key}).to_string().into_bytes()),
        ["submit", file] => {
            let bytes = fs::read(file).map_err(|_| "request file unreadable")?;
            if bytes.len() > 16384 { return Err("request too large".into()); }
            let _: rack_ai_media::types::JobRequest = serde_json::from_slice(&bytes).map_err(|_| "invalid request schema")?;
            (Method::POST, "jobs".into(), bytes)
        },
        [operation @ ("job" | "cancel" | "session" | "finish"), id] => {
            uuid::Uuid::parse_str(id).map_err(|_| "invalid resource ID")?;
            match *operation {
                "job" => (Method::GET, format!("jobs/{id}"), vec![]),
                "cancel" => (Method::POST, format!("jobs/{id}/cancel"), b"{}".to_vec()),
                "session" => (Method::GET, format!("sessions/{id}"), vec![]),
                _ => (Method::POST, format!("sessions/{id}/release"), b"{}".to_vec()),
            }
        },
        _ => return Err("usage: rack_ai_media_client status|profiles|start KEY|submit FILE|job ID|cancel ID|session ID|finish ID".into())
    };
    Ok((method, path, body))
}
