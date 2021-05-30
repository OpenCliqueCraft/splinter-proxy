use std::{
    collections::{
        HashMap,
        HashSet,
    },
    iter::FromIterator,
    sync::{
        Arc,
        RwLock,
    },
};

use craftio_rs::CraftSyncWriter;
use mcproto_rs::{
    protocol::{
        HasPacketKind,
        Id,
        PacketErr,
        RawPacket,
    },
    types::VarInt,
    v1_16_3::{
        EntityMetadataFieldData,
        PlaySpawnEntitySpec,
    },
};

use crate::{
    proto::{
        PacketLatest,
        PacketLatestKind,
        RawPacketLatest,
    },
    state::{
        EntityData,
        SplinterClient,
        SplinterServerConnection,
        SplinterState,
    },
};

pub mod eid;
pub mod uuid;

/// A packet that is lazily deserialized when the deserialized packet is accessed
pub struct LazyDeserializedPacket<'a> {
    raw_packet: Option<&'a RawPacketLatest<'a>>,
    de_packet: Option<Result<PacketLatest, PacketErr>>,
}

impl<'b> LazyDeserializedPacket<'b> {
    /// Creates a new lazy packet from a raw packet
    pub fn new(raw_packet: &'b RawPacketLatest) -> Self {
        Self {
            raw_packet: Some(raw_packet),
            de_packet: None,
        }
    }
    /// Creates a new lazy packet from an already deserialized packet
    pub fn from_packet(packet: PacketLatest) -> Self {
        Self {
            raw_packet: None,
            de_packet: Some(Ok(packet)),
        }
    }
    fn de(&mut self) {
        if self.de_packet.is_none() {
            if let Some(raw_packet) = self.raw_packet {
                self.de_packet = Some(raw_packet.deserialize());
            }
        }
    }
    /// Gets the kind of this packet
    pub fn kind(&self) -> PacketLatestKind {
        if let Some(raw_packet) = self.raw_packet {
            raw_packet.kind()
        } else {
            self.de_packet.as_ref().unwrap().as_ref().unwrap().kind()
        }
    }
    /// Gets a mutable reference to the deserialized packet. Packet may be deserialized during this
    /// call
    pub fn packet(&mut self) -> Result<&mut PacketLatest, &mut PacketErr> {
        self.de();
        let res = self.de_packet.as_mut().unwrap();
        res.as_mut()
    }
    /// Returns ownership to the deserialized packet. Packet may be deserialized during this call
    pub fn into_packet(mut self) -> Result<PacketLatest, PacketErr> {
        self.de();
        self.de_packet.unwrap()
    }
    /// Checks if this packet has an already deserialized version
    pub fn is_deserialized(&self) -> bool {
        self.de_packet.is_some()
    }
}

/// Generates IDs, and returns available IDs that have been returned
pub struct IdGenerator(Vec<u64>, HashSet<u64>);
impl IdGenerator {
    /// Creates a new id generator
    pub fn new() -> IdGenerator {
        IdGenerator(vec![1u64], HashSet::from_iter([1u64])) // this does in fact need to be set to 1, not 0
    }
    /// Takes an ID from the generator
    pub fn take_id(&mut self) -> u64 {
        if self.0.len() > 1 {
            self.0.remove(self.0.len() - 2)
        } else {
            let id = self.0.remove(0); // array should never be len of 0
            self.0.push(id + 1);
            id
        }
    }
    /// Returns an ID to the generator for reuse
    pub fn return_id(&mut self, id: u64) {
        if self.1.insert(id) {
            self.0.insert(self.0.len() - 1, id);
        }
    }
}

/// Action function called for client-proxy packets
pub type ClientPacketActionFn = Box<
    dyn Sync
        + Send
        + Fn(&Arc<SplinterClient>, &Arc<SplinterState>, &mut LazyDeserializedPacket) -> bool,
>;
/// Function for client-proxy packet mapping
pub type ClientPacketMap = PacketMap<ClientPacketActionFn>;

/// Action function called for proxy-server packets
pub type ServerPacketActionFn = Box<
    dyn Sync
        + Send
        + Fn(
            &Arc<SplinterClient>,
            &Arc<SplinterServerConnection>,
            &Arc<SplinterState>,
            &mut LazyDeserializedPacket,
        ) -> bool,
>;
/// Function for proxy-server packet mapping
pub type ServerPacketMap = PacketMap<ServerPacketActionFn>;

/// Packet map type
pub struct PacketMap<N>(pub HashMap<PacketLatestKind, Vec<N>>);

impl<N> PacketMap<N> {
    /// Adds an action to listen to the specified kind
    pub fn add_action(&mut self, kind: PacketLatestKind, action: N) {
        match self.0.get_mut(&kind) {
            Some(entry) => entry.push(action),
            None => {
                self.0.insert(kind, vec![action]);
            }
        }
    }
}
