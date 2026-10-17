use crate::types::Demand;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    Fits,
    InputLimitExceeded,
    OutputLimitExceeded,
    ContextLimitExceeded,
}

#[derive(Serialize)]
pub(crate) struct Report {
    schema: &'static str,
    service: String,
    model: String,
    reservation_id: String,
    generation: String,
    profile_hash: String,
    profile_version: String,
    input_tokens: u32,
    max_input_tokens: u32,
    max_output_tokens: u32,
    context_tokens: u32,
    requested_output_tokens: u32,
    total_reserved_tokens: u64,
    pub fits: bool,
    reason: Reason,
}

impl Report {
    pub(super) fn new(demand: &Demand, input: (u32, u32)) -> Self {
        let (requested, tokens) = input;
        let profile = &demand.profile;
        let total = u64::from(tokens) + u64::from(requested);
        let reason = if tokens > profile.max_input_tokens {
            Reason::InputLimitExceeded
        } else if requested > profile.max_output_tokens {
            Reason::OutputLimitExceeded
        } else if total > u64::from(profile.context_tokens) {
            Reason::ContextLimitExceeded
        } else {
            Reason::Fits
        };
        let fits = matches!(reason, Reason::Fits);
        Self {
            schema: "rack-ai/input-preflight/v1",
            service: profile.tag.clone(),
            model: profile.model.clone(),
            reservation_id: demand.id.clone(),
            generation: demand.generation.clone(),
            profile_hash: demand.profile_hash.clone(),
            profile_version: profile.version.clone(),
            input_tokens: tokens,
            max_input_tokens: profile.max_input_tokens,
            max_output_tokens: profile.max_output_tokens,
            context_tokens: profile.context_tokens,
            requested_output_tokens: requested,
            total_reserved_tokens: total,
            fits,
            reason,
        }
    }
}
