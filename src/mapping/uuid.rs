use mcproto_rs::uuid::UUID4;

use super::SplinterMapping;

/// Referenced <https://github.com/AdoptOpenJDK/openjdk-jdk8u/blob/9a91972c76ddda5c1ce28b50ca38cbd8a30b7a72/jdk/src/share/classes/java/util/UUID.java#L153-L175> to make this
pub fn uuid_from_bytes(bytes: &[u8]) -> UUID4 {
    let mut md5_bytes: [u8; 16] = md5::compute(bytes).into();
    md5_bytes[6] &= 0x0f;
    md5_bytes[6] |= 0x30;
    md5_bytes[8] &= 0x3f;
    md5_bytes[8] |= 0x80;
    UUID4::from(u128::from_be_bytes(md5_bytes))
}

impl SplinterMapping {
    pub fn map_uuid_server_to_proxy(&mut self, server_id: u64, server_uuid: UUID4) -> UUID4 {
        if let Some(uuid) = self.uuids.get_by_right(&(server_id, server_uuid)) {
            *uuid
        } else {
            let new_uuid = UUID4::random();
            self.uuids.insert(new_uuid, (server_id, server_uuid));
            new_uuid
        }
    }
    pub fn map_uuid_proxy_to_server(&mut self, proxy_uuid: UUID4) -> anyhow::Result<(u64, UUID4)> {
        if let Some(server_uuid_pair) = self.uuids.get_by_left(&proxy_uuid) {
            Ok(*server_uuid_pair)
        } else {
            bail!("Could not find existing mapping for uuid {}", proxy_uuid);
        }
    }
}
