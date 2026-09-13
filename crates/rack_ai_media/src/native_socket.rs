use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{Message as Upstream, client::IntoClientRequest};
pub struct SocketTarget {
    pub url: String,
    pub secret: String,
    pub browser_session: Option<crate::browser_socket::BrowserSocket>,
    pub permit: tokio::sync::OwnedSemaphorePermit,
}
pub async fn proxy(mut browser: WebSocket, target: SocketTarget) {
    let Ok(mut request) = target.url.into_client_request() else {
        return;
    };
    let Ok(secret) = target.secret.parse() else {
        return;
    };
    request.headers_mut().insert("X-Rack-Control", secret);
    let connected = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio_tungstenite::connect_async(request),
    )
    .await;
    let Ok(Ok((mut upstream, _))) = connected else {
        return;
    };
    // One outer deadline also bounds stalled socket writes.
    let _permit = target.permit;
    let browser_session = target.browser_session;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(crate::limits::SOCKET_SECONDS), async {
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(crate::limits::SOCKET_SECONDS));
    tokio::pin!(deadline);
    let mut authentication = tokio::time::interval(std::time::Duration::from_secs(
        crate::limits::BROWSER_SOCKET_AUTH_SECONDS));
    loop {
        tokio::select! {
            _=authentication.tick()=>{if !crate::browser_socket::valid(&browser_session).await {break;}},
            _=&mut deadline=>break,
            message=browser.next()=>match message{
                Some(Ok(Message::Text(s)))=>{if !crate::browser_socket::valid(&browser_session).await{break;}if upstream.send(Upstream::Text(s.to_string().into())).await.is_err(){break;}},
                Some(Ok(Message::Binary(b)))=>{if !crate::browser_socket::valid(&browser_session).await{break;}if upstream.send(Upstream::Binary(b)).await.is_err(){break;}},
                Some(Ok(Message::Ping(_)|Message::Pong(_)))=>{},
                _=>break,
            },
            message=upstream.next()=>match message{
                Some(Ok(Upstream::Text(s))) if s.len()<=crate::limits::SOCKET_MESSAGE_BYTES=>{if browser.send(Message::Text(s.to_string().into())).await.is_err(){break;}},
                Some(Ok(Upstream::Binary(b))) if b.len()<=crate::limits::SOCKET_MESSAGE_BYTES=>{if browser.send(Message::Binary(b)).await.is_err(){break;}},
                Some(Ok(Upstream::Ping(_)|Upstream::Pong(_)))=>{},
                _=>break,
            }
        }
    }
    }).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), browser.close()).await;
}
