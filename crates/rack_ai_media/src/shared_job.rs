use crate::{
    config::{Config, Principal},
    store::Store,
    types::{JobRequest, Mode},
};
use rack_ai_application::LeaseHandle;
use rack_ai_infrastructure::{
    managed_lease::ManagedLease, resource_reservations::ResourceReservations,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaReservation {
    pub id: String,
    pub generation: String,
}
pub struct SharedJob<'a> {
    pub config: &'a Config,
    pub store: &'a Store,
}
impl SharedJob<'_> {
    pub fn authorize(&self, principal: &Principal, request: &JobRequest) -> Result<(), String> {
        let state = self.store.read()?;
        let resources = ResourceReservations::new(self.config.resource_root.clone());
        let managed = state
            .service
            .lease
            .as_ref()
            .map(|h| {
                (ManagedLease {
                    resources: &resources,
                })
                .verify(h, false)
            })
            .transpose()?
            .unwrap_or(false);
        let Some(binding) = &request.reservation else {
            return if managed {
                Err("forbidden: managed reservation binding required".into())
            } else {
                Ok(())
            };
        };
        let handle = LeaseHandle {
            owner: binding.id.clone(),
            generation: binding.generation.clone(),
            paths: [(
                "gpu-4080-super".into(),
                resources
                    .path("gpu-4080-super")?
                    .to_string_lossy()
                    .into_owned(),
            )]
            .into_iter()
            .collect(),
        };
        ManagedLease {
            resources: &resources,
        }
        .authorize(&handle, (&principal.id, request.priority))?;
        if state.service.mode != Mode::Managed
            || state
                .service
                .lease
                .as_ref()
                .is_none_or(|h| h.owner != handle.owner || h.generation != handle.generation)
        {
            return Err("conflict: media reservation not active".into());
        }
        Ok(())
    }
}
