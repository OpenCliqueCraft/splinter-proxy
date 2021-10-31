use std::{
    collections::{
        HashMap,
        HashSet,
    },
    iter::FromIterator,
};

use bimap::BiHashMap;
use mcproto_rs::uuid::UUID4;

pub struct SplinterMapping {
    pub uuids: BiHashMap<UUID4, (u64, UUID4)>,
    pub eids: BiHashMap<i32, (u64, i32)>,
    pub entity_data: HashMap<i32, EntityData>,
    pub eid_gen: IdGenerator,
}

impl SplinterMapping {
    pub fn new() -> Self {
        Self {
            uuids: BiHashMap::new(),
            eids: BiHashMap::new(),
            eid_gen: IdGenerator::new(),
            entity_data: HashMap::new(),
        }
    }
    pub fn register_eid_mapping(&mut self, server_id: u64, server_eid: i32) -> i32 {
        let new_eid = self.eid_gen.take_id() as i32;
        self.eids.insert(new_eid, (server_id, server_eid));
        // debug!("New mapping s->p eid {} to {}", server_eid, new_eid);
        new_eid
    }
    pub fn register_uuid_mapping(&mut self, server_id: u64, server_uuid: UUID4) -> UUID4 {
        let new_uuid = UUID4::random();
        self.uuids.insert(new_uuid, (server_id, server_uuid));
        new_uuid
    }
}

pub enum SplinterMappingResult {
    Server(u64),
    Client,
    None,
}

/// Referenced <https://github.com/AdoptOpenJDK/openjdk-jdk8u/blob/9a91972c76ddda5c1ce28b50ca38cbd8a30b7a72/jdk/src/share/classes/java/util/UUID.java#L153-L175> to make this
pub fn uuid_from_bytes(bytes: &[u8]) -> UUID4 {
    let mut md5_bytes: [u8; 16] = md5::compute(bytes).into();
    md5_bytes[6] &= 0x0f;
    md5_bytes[6] |= 0x30;
    md5_bytes[8] &= 0x3f;
    md5_bytes[8] |= 0x80;
    UUID4::from(u128::from_be_bytes(md5_bytes))
}

pub fn uuid_from_name(name: impl AsRef<str>) -> UUID4 {
    uuid_from_bytes(format!("OfflinePlayer:{}", name.as_ref()).as_bytes())
}

pub struct EntityData {
    pub id: i32,
    pub entity_type: i32,
}

pub struct IdGenerator {
    available_ids: Vec<u64>,
    available_ids_set: HashSet<u64>,
}
impl IdGenerator {
    pub fn new() -> Self {
        const INITIAL_ID: u64 = 1;
        Self {
            available_ids: vec![INITIAL_ID],
            available_ids_set: HashSet::from_iter([INITIAL_ID]),
        }
    }
    pub fn take_id(&mut self) -> u64 {
        if self.available_ids.len() > 1 {
            self.available_ids.remove(self.available_ids.len() - 2) // remove second to last
        } else {
            let id = self.available_ids.remove(0);
            self.available_ids.push(id + 1);
            id
        }
    }
    pub fn return_id(&mut self, id: u64) {
        if self.available_ids_set.insert(id) {
            self.available_ids.insert(self.available_ids.len() - 1, id);
        }
    }
}
