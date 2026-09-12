use crate::{
    config::Principal,
    types::{Mode, ServiceState},
    web_state::{WebState, blocking},
};
use axum::{
    extract::{FromRequestParts, Request, State, ws::WebSocketUpgrade},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
pub async fn handle(State(app): State<WebState>, request: Request) -> Response {
    let Some(principal) = request.extensions().get::<Principal>().cloned() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let read = app.clone();
    let result = blocking(move || {
        let state = read.store.read()?;
        let owned = state.sessions.iter().any(|s| {
            Some(&s.id) == state.service.session.as_ref()
                && s.owner == principal.id
                && !s.release_requested
        });
        if !owned
            || state.service.mode != Mode::Interactive
            || state.service.state != ServiceState::Ready
        {
            return Err("interactive session is not ready".into());
        }
        std::fs::read_to_string(&read.config.control_secret_file).map_err(|e| e.to_string())
    })
    .await;
    let secret=match result{Ok(s)=>s.trim().to_string(),Err(_)=>return (StatusCode::CONFLICT,
        axum::response::Html("<h1>ComfyUI session is not ready</h1><p>Use the Rack AI launcher to start or inspect your session.</p>")).into_response()};
    if request
        .headers()
        .get(header::UPGRADE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.eq_ignore_ascii_case("websocket"))
    {
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
    let url = format!(
        "{}{}",
        app.config.backend.trim_end_matches('/'),
        request
            .uri()
            .path_and_query()
            .map(|p| p.as_str())
            .unwrap_or("/")
    );
    let method = request.method().clone();
    let content_type = request.headers().get(header::CONTENT_TYPE).cloned();
    let body =
        match axum::body::to_bytes(request.into_body(), crate::limits::NATIVE_UPLOAD_BYTES).await {
            Ok(b) => b,
            Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        };
    let mut outgoing = app
        .client
        .request(method, url)
        .header("X-Rack-Control", secret)
        .header("X-Rack-Access", "interactive")
        .body(body);
    if let Some(content_type) = content_type {
        outgoing = outgoing.header(header::CONTENT_TYPE, content_type);
    }
    let upstream = match outgoing.send().await {
        Ok(r) => r,
        Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
    };
    if upstream
        .content_length()
        .is_some_and(|n| n > crate::limits::NATIVE_RESPONSE_BYTES as u64)
    {
        return StatusCode::BAD_GATEWAY.into_response();
    }
    let status = upstream.status();
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    use futures_util::StreamExt;
    let mut stream = upstream.bytes_stream();
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(b) if data.len() + b.len() <= crate::limits::NATIVE_RESPONSE_BYTES => {
                data.extend_from_slice(&b)
            }
            _ => return StatusCode::BAD_GATEWAY.into_response(),
        }
    }
    let mut response = (status, data).into_response();
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    response
}
