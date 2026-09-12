use crate::{backend::Backend, config::Config, store::Store, systemd::Systemd};
use rack_ai_infrastructure::resource_reservations::ResourceReservations;
pub struct Runtime {
    pub config: Config,
    pub store: Store,
    pub backend: Backend,
    pub systemd: Systemd,
    pub reservations: ResourceReservations,
}
impl Runtime {
    pub fn new(config: Config) -> Result<Self, String> {
        Ok(Self {
            store: Store::new(config.state_root.clone()),
            backend: Backend::new(&config)?,
            systemd: Systemd::new(&config),
            reservations: ResourceReservations::new(config.resource_root.clone()),
            config,
        })
    }
}
