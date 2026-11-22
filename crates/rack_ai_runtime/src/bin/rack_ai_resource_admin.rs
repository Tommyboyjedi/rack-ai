//! Local operator bridge; it never opens a listener or dispatches work.
use rack_ai_runtime::{config::Config, resource_recovery, service::Service};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [operation, config] = args.as_slice() else {
        return Err("usage: rack_ai_resource_admin inspect|recover ADMIN_CONFIG".into());
    };
    let apply = match operation.as_str() {
        "inspect" => false,
        "recover" => true,
        _ => return Err("unknown resource recovery operation".into()),
    };
    let service = Service::new(Config::load(std::path::Path::new(config))?);
    let report = resource_recovery::run(&service, apply)?;
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}
