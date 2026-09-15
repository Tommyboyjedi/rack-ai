//! Binary results live in the private authority directory, never base64 JSON.
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Deserialize)]
pub struct Health {
    pub model: String,
    pub activation: String,
    pub sample_rate: u32,
    pub voices: Vec<String>,
}
#[derive(Serialize, Deserialize)]
pub struct Receipt {
    pub sha256: String,
    pub bytes: u64,
}
fn client(seconds: u64) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(seconds))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| "speech_client".into())
}
pub fn health(d: &Demand) -> Result<Health, String> {
    crate::process::endpoint_owned(
        d.process.as_ref().ok_or("activation_process_missing")?,
        &d.profile,
    )?;
    let response = client(2)?
        .get(format!("{}/health", d.profile.endpoint))
        .bearer_auth(&d.generation)
        .send()
        .map_err(|_| "speech_health_unavailable")?;
    if !response.status().is_success() {
        return Err("speech_health_failure".into());
    }
    let mut data = Vec::new();
    response
        .take(16385)
        .read_to_end(&mut data)
        .map_err(|_| "speech_health_read")?;
    if data.len() > 16384 {
        return Err("speech_health_bounds".into());
    }
    let h: Health = serde_json::from_slice(&data).map_err(|_| "speech_health_invalid")?;
    if h.model != d.profile.model
        || h.activation != d.generation
        || h.sample_rate != 24000
        || h.voices.is_empty()
        || h.voices.len() > 128
        || h.voices.iter().any(|v| !valid_id(v))
    {
        return Err("speech_identity_mismatch".into());
    }
    Ok(h)
}
pub fn ready(d: &Demand) -> Result<(), String> {
    health(d).map(|_| ())
}
pub fn synthesize(
    context: (&Demand, &Invocation),
    root: &Path,
) -> Result<serde_json::Value, String> {
    let (d, i) = context;
    let payload = i.request.payload.as_ref().ok_or("missing_speech")?;
    crate::speech::validate(payload, d)?;
    let response = client(i.request.timeout_seconds)?
        .post(format!("{}/speech", d.profile.endpoint))
        .bearer_auth(&d.generation)
        .json(&payload.body)
        .send()
        .map_err(|_| "speech_transport_uncertain")?;
    if !response.status().is_success()
        || response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            != Some("audio/wav")
    {
        return Err("speech_response_invalid".into());
    }
    let mut bytes = Vec::new();
    response
        .take(i.response_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "speech_read_uncertain")?;
    wav(&bytes)?;
    let receipt = Receipt {
        sha256: digest(&bytes),
        bytes: bytes.len() as u64,
    };
    let destination = path(root, &i.id)?;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let directory = destination.parent().ok_or("speech_directory")?;
    std::fs::create_dir_all(directory).map_err(|_| "speech_storage")?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| "speech_storage")?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&destination)
        .map_err(|_| "speech_storage")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "speech_storage")?;
    std::fs::File::open(directory)
        .and_then(|f| f.sync_all())
        .map_err(|_| "speech_storage")?;
    Ok(serde_json::json!({"rack_protocol_response":{"content_type":"audio/wav","audio":receipt}}))
}
fn path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) || id.contains("..") {
        return Err("invalid_audio_identity".into());
    }
    Ok(root.join("speech").join(format!("{id}.wav")))
}
pub fn read(root: &Path, i: &Invocation) -> Result<Vec<u8>, String> {
    let receipt: Receipt = serde_json::from_value(
        i.result.as_ref().ok_or("missing_speech_result")?["rack_protocol_response"]["audio"]
            .clone(),
    )
    .map_err(|_| "invalid_speech_receipt")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path(root, &i.id)?)
        .map_err(|_| "speech_result_unavailable")?
        .take(crate::speech::MAX_WAV_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "speech_result_unavailable")?;
    if bytes.len() as u64 != receipt.bytes || digest(&bytes) != receipt.sha256 {
        return Err("speech_integrity_failure".into());
    }
    wav(&bytes)?;
    Ok(bytes)
}
pub fn wav(b: &[u8]) -> Result<(), String> {
    // Worker emits canonical PCM16 WAV; do not accept ambiguous chunks or compressed data.
    if b.len() < 46
        || b.len() as u64 > crate::speech::MAX_WAV_BYTES
        || &b[0..4] != b"RIFF"
        || &b[8..16] != b"WAVEfmt "
        || &b[36..40] != b"data"
        || b[16..36]
            != [
                16, 0, 0, 0, 1, 0, 1, 0, 192, 93, 0, 0, 128, 187, 0, 0, 2, 0, 16, 0,
            ]
        || u32::from_le_bytes(b[4..8].try_into().map_err(|_| "wav_header")?) as usize + 8 != b.len()
        || u32::from_le_bytes(b[40..44].try_into().map_err(|_| "wav_header")?) as usize + 44
            != b.len()
        || !(b.len() - 44).is_multiple_of(2)
    {
        return Err("invalid_bounded_pcm24k_wav".into());
    }
    Ok(())
}
