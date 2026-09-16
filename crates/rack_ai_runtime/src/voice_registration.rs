use crate::{service::Service, voice_registry::Upload};
use axum::{
    Json,
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
pub const MAX_FORM_BYTES: usize = crate::reference_audio::MAX_BYTES + 16384;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);
pub async fn handle(
    State(service): State<Arc<Service>>,
    form: Result<Multipart, axum::extract::multipart::MultipartRejection>,
) -> Response {
    let Ok(_permit) = service.admission_slots.clone().try_acquire_owned() else {
        return failure("voice_registration_busy".into());
    };
    let path = match crate::voice_registry::configured_path(&service.config) {
        Ok(path) => path,
        Err(error) => return failure(error),
    };
    let form = match form {
        Ok(form) => form,
        Err(_) => return failure("invalid_voice_multipart".into()),
    };
    let upload = match tokio::time::timeout(UPLOAD_TIMEOUT, read(form)).await {
        Ok(Ok(upload)) => upload,
        Ok(Err(error)) => return failure(error),
        Err(_) => return failure("voice_upload_timeout".into()),
    };
    match tokio::task::spawn_blocking(move || crate::voice_registry::register(&path, upload)).await
    {
        Ok(Ok(registered)) => Json(registered).into_response(),
        Ok(Err(error)) => failure(error),
        Err(_) => failure("voice_registry_unavailable".into()),
    }
}
async fn read(mut form: Multipart) -> Result<Upload, String> {
    let mut voice_id = None;
    let mut audio = None;
    while let Some(mut field) = form.next_field().await.map_err(multipart_error)? {
        let name = field.name().ok_or("invalid_voice_multipart")?.to_string();
        let bound = match name.as_str() {
            "voice_id" if voice_id.is_none() && field.file_name().is_none() => 128,
            "file" if audio.is_none() => {
                let filename = field.file_name().ok_or("invalid_voice_filename")?;
                if filename.len() > 128
                    || !filename.to_ascii_lowercase().ends_with(".wav")
                    || !crate::voice_registry::safe_id(filename)
                {
                    return Err("invalid_voice_filename".into());
                }
                crate::reference_audio::MAX_BYTES
            }
            _ => return Err("invalid_voice_multipart".into()),
        };
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
            if bytes.len() + chunk.len() > bound {
                return Err(if name == "file" {
                    "voice_file_too_large"
                } else {
                    "invalid_voice_id"
                }
                .into());
            }
            bytes.extend_from_slice(&chunk);
        }
        if name == "voice_id" {
            let id = String::from_utf8(bytes).map_err(|_| "invalid_voice_id")?;
            if !crate::voice_registry::safe_id(&id) {
                return Err("invalid_voice_id".into());
            }
            voice_id = Some(id);
        } else {
            audio = Some(bytes);
        }
    }
    Ok(Upload {
        voice_id: voice_id.ok_or("invalid_voice_multipart")?,
        bytes: audio.ok_or("invalid_voice_multipart")?,
    })
}
fn multipart_error(error: axum::extract::multipart::MultipartError) -> String {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        "voice_file_too_large"
    } else {
        "invalid_voice_multipart"
    }
    .into()
}
fn failure(error: String) -> Response {
    let status = match error.as_str() {
        "voice_file_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
        "voice_registration_busy" | "voice_registry_full" => StatusCode::TOO_MANY_REQUESTS,
        "voice_upload_timeout" => StatusCode::REQUEST_TIMEOUT,
        e if e.starts_with("invalid_")
            || e == "unsupported_reference_format"
            || e == "reference_duration_bounds" =>
        {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    (status, Json(json!({"error":error}))).into_response()
}
