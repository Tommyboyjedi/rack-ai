use crate::{native::AuthorizedNative, web_state::WebState};
use axum::{
    extract::{FromRequestParts, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
};
pub async fn handle(app: WebState, input: AuthorizedNative) -> Response {
    let AuthorizedNative { request, secret } = input;
    if !crate::auth::origin_allowed(request.headers(), &app) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(permit) = app.sockets.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (mut parts, _) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    let ws = match WebSocketUpgrade::from_request_parts(&mut parts, &app).await {
        Ok(ws) => ws,
        Err(e) => return e.into_response(),
    };
    return ws
        .max_message_size(crate::limits::SOCKET_MESSAGE_BYTES)
        .max_frame_size(crate::limits::SOCKET_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            crate::native_socket::proxy(
                socket,
                crate::native_socket::SocketTarget {
                    url: format!(
                        "ws://{}{}",
                        app.config
                            .backend
                            .trim_start_matches("http://")
                            .trim_end_matches('/'),
                        path
                    ),
                    secret,
                    permit,
                },
            )
        })
        .into_response();
}
