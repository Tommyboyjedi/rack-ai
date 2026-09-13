//! Cookie revocation applies to already-open native browser connections as well.
use crate::{
    browser_sessions,
    web_state::{WebState, blocking},
};
pub struct BrowserSocket {
    pub app: WebState,
    pub digest: String,
}
impl BrowserSocket {
    pub async fn valid(&self) -> bool {
        let app = self.app.clone();
        let digest = self.digest.clone();
        blocking(move || browser_sessions::principal(&app, &digest).map(|_| ()))
            .await
            .is_ok()
    }
}
pub async fn valid(browser: &Option<BrowserSocket>) -> bool {
    match browser {
        Some(browser) => browser.valid().await,
        None => true,
    }
}
