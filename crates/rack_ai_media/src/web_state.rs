use crate::{config::Config, store::Store};
use std::sync::Arc;
#[derive(Clone)]
pub struct WebState {
    pub config: Arc<Config>,
    pub store: Store,
    pub client: reqwest::Client,
    pub connections: Arc<tokio::sync::Semaphore>,
    pub sockets: Arc<tokio::sync::Semaphore>,
}
impl WebState {
    pub fn new(config: Config) -> Result<Self, String> {
        Ok(Self {
            store: Store::new(config.state_root.clone()),
            config: Arc::new(config),
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(20))
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| e.to_string())?,
            connections: Arc::new(tokio::sync::Semaphore::new(crate::limits::HTTP_CONCURRENCY)),
            sockets: Arc::new(tokio::sync::Semaphore::new(
                crate::limits::SOCKET_CONCURRENCY,
            )),
        })
    }
}
pub async fn blocking<T: Send + 'static>(
    action: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(action)
        .await
        .map_err(|_| "media operation interrupted".to_string())?
}
