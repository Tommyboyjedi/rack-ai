//! The configured loopback URL must agree with the actual hosting listener.
use crate::config::Profile;
pub fn private_binding(p: &Profile) -> Result<(), String> {
    let mut hosts = p.args.iter().enumerate().filter_map(|(n, arg)| {
        if arg == "--host" {
            Some(p.args.get(n + 1).map(String::as_str).unwrap_or(""))
        } else {
            arg.strip_prefix("--host=")
        }
    });
    let host = hosts.next().ok_or("backend_loopback_bind_required")?;
    if !matches!(host, "127.0.0.1" | "::1") || hosts.next().is_some() {
        return Err("backend_loopback_bind_required".into());
    }
    let endpoint = reqwest::Url::parse(&p.endpoint).map_err(|_| "invalid endpoint")?;
    if endpoint.host_str().map(|h| h.trim_matches(['[', ']'])) != Some(host) {
        return Err("backend_bind_endpoint_mismatch".into());
    }
    Ok(())
}
