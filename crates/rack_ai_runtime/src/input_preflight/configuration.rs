//! Preserve legacy counters unless a profile explicitly qualifies native media counting.
use crate::config::{Backend, Profile};
use crate::protocol::Protocol;

pub(crate) fn validate(profile: &Profile) -> Result<(), String> {
    if profile.native_multimodal_counter()
        && (!profile.input_token_preflight
            || !matches!(profile.backend, Backend::LlamaCpp | Backend::Strata)
            || !profile.protocols.contains(&Protocol::ChatCompletions))
    {
        return Err("native multimodal counter requires qualified input preflight".into());
    }
    if profile.enforce_input_limits
        && profile.max_images_per_request > 0
        && !profile.native_multimodal_counter()
    {
        return Err("image input enforcement requires native multimodal counting".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InputTokenCounter;
    #[test]
    fn omitted_counter_preserves_legacy_serialization_and_explicit_selector_is_typed() {
        let mut profile: Profile = serde_json::from_str(include_str!(
            "../../../../config/runtime/profiles/tiny-uncd.json"
        ))
        .unwrap();
        let original = serde_json::to_value(&profile).unwrap();
        assert!(original.get("input_token_counter").is_none());
        profile.input_token_counter = Some(InputTokenCounter::NativeMultimodal);
        assert!(validate(&profile).is_ok());
        let mut explicit = serde_json::to_value(&profile).unwrap();
        assert_eq!(explicit["input_token_counter"], "native_multimodal");
        explicit
            .as_object_mut()
            .unwrap()
            .remove("input_token_counter");
        assert_eq!(explicit, original);
        profile.input_token_preflight = false;
        assert!(validate(&profile).is_err());
        profile.input_token_preflight = true;
        profile.backend = Backend::Vllm;
        assert!(validate(&profile).is_err());
    }
}
