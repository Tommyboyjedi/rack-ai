use crate::{native::AuthorizedNative, web_state::WebState};
use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
pub async fn handle(app: WebState, input: AuthorizedNative) -> Response {
    let AuthorizedNative { request, secret } = input;
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
