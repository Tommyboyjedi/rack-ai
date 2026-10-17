//! Optional sampler extensions are accepted only on their qualified backend.
use super::{Payload, Protocol};
use crate::config::Backend;

pub(super) fn validate(payload: &Payload, backend: &Backend) -> Result<(), String> {
    let body = &payload.body;
    if body.get("top_k").is_none() && body.get("min_p").is_none() {
        return Ok(());
    }
    if payload.protocol != Protocol::ChatCompletions
        || !matches!(backend, Backend::LlamaCpp | Backend::Strata)
    {
        return Err("sampler_options_unqualified".into());
    }
    if body
        .get("top_k")
        .is_some_and(|value| value.as_u64().is_none_or(|number| number > i32::MAX as u64))
        || body.get("min_p").is_some_and(|value| {
            value
                .as_f64()
                .is_none_or(|number| !number.is_finite() || !(0.0..=1.0).contains(&number))
        })
    {
        return Err("invalid_sampler_options".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn payload(body: Value) -> Payload {
        Payload {
            protocol: Protocol::ChatCompletions,
            body,
        }
    }

    #[test]
    fn preserves_tested_sampling_and_accepts_disabled_filters() {
        for body in [
            json!({"top_k":40,"min_p":0}),
            json!({"top_k":0,"min_p":1.0}),
        ] {
            let value = payload(body);
            let original = value.body.clone();
            assert!(validate(&value, &Backend::LlamaCpp).is_ok());
            assert!(validate(&value, &Backend::Strata).is_ok());
            assert_eq!(value.body, original);
        }
    }

    #[test]
    fn rejects_wrong_types_out_of_range_and_unqualified_backends() {
        for body in [
            json!({"top_k":-1}),
            json!({"top_k":1.5}),
            json!({"top_k":null}),
            json!({"top_k":2147483648u64}),
            json!({"min_p":-0.1}),
            json!({"min_p":1.1}),
            json!({"min_p":"0"}),
            json!({"min_p":null}),
        ] {
            assert_eq!(
                validate(&payload(body), &Backend::LlamaCpp).unwrap_err(),
                "invalid_sampler_options"
            );
        }
        for backend in [Backend::Vllm, Backend::Comfyui, Backend::Chatterbox] {
            assert_eq!(
                validate(&payload(json!({"top_k":40})), &backend).unwrap_err(),
                "sampler_options_unqualified"
            );
        }
        let mut value = payload(json!({"min_p":0}));
        value.protocol = Protocol::Responses;
        assert!(validate(&value, &Backend::LlamaCpp).is_err());
    }
}
