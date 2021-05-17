use std::{
    collections::HashMap,
    sync::Arc,
};

use mcproto_rs::{
    protocol::{
        HasPacketKind,
        Id,
    },
    v1_16_3::{
        Packet753 as PacketLatest,
        Packet753Kind as PacketLatestKind,
        RawPacket753 as RawPacketLatest,
    },
};

/// Specifies what to do with a packet
pub enum MapAction<'a> {
    /// Relay the packet the same direction it was already heading.
    Relay(RawPacketLatest<'a>),

    /// Send the packet to a server
    Server(PacketLatest), // TODO: will have to do like a server id or something

    /// Send the packet to a client
    Client(PacketLatest),

    /// Don't do anything
    None,
}

/// The function type for packet mapping
pub type PacketMapFn = Box<dyn Sync + Send + Fn(RawPacketLatest) -> MapAction>;

/// Packet map type
pub type PacketMap = HashMap<PacketLatestKind, PacketMapFn>;

/// Maps a packet to its corresponding type's function
///
/// `map` is the [`PacketMap`] to find the corresponding function from. `raw_packet` is the packet
/// to check against and pass into the function
pub fn process_raw_packet<'a>(
    map: &'a PacketMap,
    raw_packet: RawPacketLatest<'a>,
) -> MapAction<'a> {
    return match map.get(&raw_packet.kind()) {
        Some(entry) => entry(raw_packet),
        None => MapAction::Relay::<'a>(raw_packet),
    };
}
