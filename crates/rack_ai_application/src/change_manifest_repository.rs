use crate::ReviewPacket;

pub trait ChangeManifestRepository {
    fn save(&self, packet: &ReviewPacket) -> Result<String, String>;
}
