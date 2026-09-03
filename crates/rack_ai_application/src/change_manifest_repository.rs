use crate::GenericRoutingHeader;
use crate::ReviewPacket;

pub trait ChangeManifestRepository {
    fn save(&self, packet: &ReviewPacket) -> Result<String, String>;

    fn has_idempotent_submission(&self, _header: &GenericRoutingHeader) -> Result<bool, String> {
        Ok(false)
    }
}
