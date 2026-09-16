//! Surface the existing media-native interface; do not duplicate its transport or authority.
use crate::types::*;
use serde_json::{Value, json};
pub fn describe(d: &Demand) -> Result<Value, String> {
    let mut url = None;
    let mut session_id = None;
    if d.state == DemandState::Ready
        && crate::service::active(d)
        && d.profile.media_config.is_some()
    {
        let config = crate::media_limits::configuration(d)?;
        let state = rack_ai_media::store::Store::new(config.state_root).read()?;
        if state
            .service
            .lease
            .as_ref()
            .is_some_and(|h| h.owner == d.id && h.generation == d.generation)
            && let Some(session) = state.sessions.iter().find(|session| {
                Some(&session.id) == state.service.session.as_ref()
                    && session.owner == d.owner
                    && !session.stopped
                    && !session.release_requested
            })
        {
            url = Some(config.native_origin);
            session_id = Some(session.id.clone());
        }
    }
    Ok(
        json!({"kind":"native_comfyui","url":url,"session_id":session_id,
        "reservation_id":d.id,"generation":d.generation}),
    )
}
