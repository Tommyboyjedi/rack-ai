use axum::{
    Router, middleware,
    routing::{any, get},
};
use rack_ai_media::{config::Config, runtime::Runtime, store::Store, web_state::WebState};
fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let config = Config::load(args.next().ok_or("usage: rack_ai_media CONFIG")?.into())?;
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    Store::new(config.state_root.clone()).initialize()?;
    rack_ai_media::profile::verify_checkpoint(&config.profile)?;
    let owner = rack_ai_infrastructure::resource_reservations::bounded_lock(
        &config.state_root.join("supervisor.lock"),
    )?;
    let runtime = Runtime::new(config.clone())?;
    // Reconciliation finishes before HTTP admission is made available.
    if let Err(error) = rack_ai_media::supervisor::recover(&runtime) {
        rack_ai_media::supervisor::quarantine(&runtime, &error)?;
    }
    std::thread::spawn(move || {
        let _owner = owner;
        if let Err(e) = rack_ai_media::supervisor::run(runtime) {
            eprintln!("media supervisor stopped: {e}");
            std::process::exit(1);
        }
    });
    tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())?
        .block_on(serve(config))
}
async fn serve(config: Config) -> Result<(), String> {
    let api_addr = config.listen;
    let native_addr = config.native_listen;
    let app = WebState::new(config)?;
    let api = Router::new()
        .route(
            "/",
            get(|| async { axum::response::Html(include_str!("../web/launcher.html")) }),
        )
        .route("/login", any(rack_ai_media::auth::login))
        .route(
            "/api/media/v1/jobs/{job}/artifacts/{artifact}",
            get(rack_ai_media::download::handle),
        )
        .route("/api/media/v1/{*path}", any(rack_ai_media::api::handle))
        .layer(middleware::from_fn_with_state(
            app.clone(),
            rack_ai_media::auth::guard,
        ))
        .with_state(app.clone());
    let native = Router::new()
        .route("/login", any(rack_ai_media::auth::login))
        .fallback(rack_ai_media::native::handle)
        .layer(middleware::from_fn_with_state(
            app.clone(),
            rack_ai_media::auth::guard,
        ))
        .with_state(app);
    let api = tower_timeout(api);
    let native = tower_timeout(native);
    let first = tokio::net::TcpListener::bind(api_addr)
        .await
        .map_err(|e| e.to_string())?;
    let second = tokio::net::TcpListener::bind(native_addr)
        .await
        .map_err(|e| e.to_string())?;
    tokio::try_join!(
        async { axum::serve(first, api).await.map_err(|e| e.to_string()) },
        async { axum::serve(second, native).await.map_err(|e| e.to_string()) }
    )?;
    Ok(())
}
fn tower_timeout(router: Router) -> Router {
    router.layer(tower_http::timeout::TimeoutLayer::with_status_code(
        axum::http::StatusCode::REQUEST_TIMEOUT,
        std::time::Duration::from_secs(30),
    ))
}
