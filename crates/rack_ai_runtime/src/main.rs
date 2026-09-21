use rack_ai_runtime::{api, config::Config, service::Service};
use std::sync::Arc;
#[tokio::main]
async fn main() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|arg| arg == "__sandbox-tcp-bridge")
    {
        return rack_ai_infrastructure::sandbox_tcp_bridge::run_from_args(&arguments[1..])
            .map(|_| ());
    }
    let (path, validate_only) = match arguments.as_slice() {
        [path] => (path, false),
        [operation, path] if operation == "validate" => (path, true),
        _ => return Err("usage: rack_ai_runtime [validate] ADMIN_CONFIG.json".into()),
    };
    let config = Config::load(std::path::Path::new(path))?;
    if validate_only {
        println!("RUNTIME_CONFIG_VALID");
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .map_err(|e| e.to_string())?;
    let _receiver = rack_ai_infrastructure::resource_reservations::bounded_lock(
        &config.authority_root.join("receiver.lock"),
    )?;
    let service = Arc::new(Service::new(config));
    service.recover()?;
    let supervisor = rack_ai_runtime::supervisor::Supervisor {
        service: Arc::clone(&service),
    };
    std::thread::spawn(move || {
        loop {
            if let Err(e) = supervisor.tick() {
                eprintln!("authority tick failed: {e}");
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    let workspace_recovery = rack_ai_runtime::workspace_recovery::Monitor {
        service: Arc::clone(&service),
    };
    std::thread::spawn(move || {
        workspace_recovery.run_forever();
    });
    axum::serve(listener, api::router(service))
        .await
        .map_err(|e| e.to_string())
}
