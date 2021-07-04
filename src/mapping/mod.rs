use std::{
    collections::{
        HashMap,
        HashSet,
    },
    iter::FromIterator,
};

use bimap::BiHashMap;
use mcproto_rs::uuid::UUID4;

pub mod eid;
pub mod uuid;

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
