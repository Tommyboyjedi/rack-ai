//! Fixed v1 speech contract. All generation policy is server owned.
use crate::{
    config::{Backend, Profile},
    protocol::{Payload, Protocol},
    types::*,
};
use serde::{Deserialize, Serialize};
pub const MAX_WAV_BYTES: u64 = 2_880_044;
pub const MAX_TEXT_BYTES: usize = 4096;
pub const MAX_TEXT_CHARS: usize = 1000;
pub const MAX_SECONDS: u64 = 60;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Speech {
    pub text: String,
    pub voice: String,
}
pub fn validate(payload: &Payload, d: &Demand) -> Result<u32, String> {
    let request: Speech =
        serde_json::from_value(payload.body.clone()).map_err(|_| "invalid_speech_request")?;
    if d.profile.backend != Backend::Chatterbox || !d.profile.protocols.contains(&Protocol::Speech)
    {
        return Err("speech_unqualified".into());
    }
    if !valid_id(&request.voice) || request.voice.contains("..") {
        return Err("invalid_voice_id".into());
    }
    if request.text.trim().is_empty()
        || request.text.len() > MAX_TEXT_BYTES
        || request.text.chars().count() > MAX_TEXT_CHARS
        || request.text.contains('\0')
    {
        return Err("speech_text_bounds".into());
    }
    Ok(1)
}
pub fn profile(p: &Profile) -> Result<(), String> {
    if p.backend != Backend::Chatterbox {
        return if p.protocols.contains(&Protocol::Speech) {
            Err("speech_backend_required".into())
        } else {
            Ok(())
        };
    }
    if p.capabilities != vec![rack_ai_application::GenericCapability::Audio]
        || p.protocols != vec![Protocol::Speech]
        || p.streaming
        || p.resources != vec!["gpu-2060"]
        || p.inference_seconds > MAX_SECONDS
        || p.artifact.is_none()
        || p.artifact_sha256.as_ref().is_none_or(|h| h.len() != 64)
    {
        return Err("invalid_chatterbox_profile".into());
    }
    Ok(())
}
pub fn admit(s: &Document, context: (&Demand, &Inference)) -> Result<(), String> {
    let (d, request) = context;
    const RETAINED_AUDIO_BYTES: u64 = 256 * 1024 * 1024;
    let reserved = s
        .data
        .invocations
        .values()
        .filter(|i| {
            i.request
                .payload
                .as_ref()
                .is_some_and(|p| p.protocol == Protocol::Speech)
        })
        .count() as u64
        * MAX_WAV_BYTES;
    if reserved + MAX_WAV_BYTES > RETAINED_AUDIO_BYTES {
        return Err("capacity_speech_retention".into());
    }
    if d.state != DemandState::Ready || !crate::service::owns(s, d) {
        return Err("reservation_not_dispatchable".into());
    }
    let payload = request.payload.as_ref().ok_or("use_speech_interface")?;
    if payload.protocol != Protocol::Speech {
        return Err("use_speech_interface".into());
    }
    validate(payload, d)?;
    if s.data.invocations.values().any(|i| {
        i.request.reservation_id == d.id
            && matches!(
                i.state,
                InvocationState::Accepted | InvocationState::Started | InvocationState::Uncertain
            )
    }) {
        return Err("capacity_speech_active".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn profile_is_scoped_and_exact() {
        let mut c: crate::config::Config =
            serde_json::from_str(include_str!("../../../config/runtime/config.example.json"))
                .unwrap();
        crate::validation::validate(&c).unwrap();
        c.devices.get_mut("gpu-2060").unwrap().uuid =
            "GPU-042e18f2-bf9f-c8f6-6975-6f25b15ac71d".into();
        assert_eq!(
            crate::validation::validate(&c).unwrap_err(),
            "chatterbox_exact_uuid_required"
        );
        c.devices.get_mut("gpu-2060").unwrap().uuid =
            "GPU-357ef569-8fac-7c7d-ee1c-51677efb174f".into();
        let p = c
            .profiles
            .iter_mut()
            .find(|p| p.tag == "local-tts")
            .unwrap();
        p.protocols = vec![crate::protocol::Protocol::ChatCompletions];
        assert_eq!(
            crate::validation::validate(&c).unwrap_err(),
            "invalid_chatterbox_profile"
        );
    }
}
