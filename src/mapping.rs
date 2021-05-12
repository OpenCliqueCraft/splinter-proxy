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

pub enum MapAction<'a> {
    Relay(RawPacketLatest<'a>),
    Server(PacketLatest), // TODO: will have to do like a server id or something
    Client(PacketLatest),
    None,
}

pub type PacketMapFn = Box<dyn Sync + Send + Fn(RawPacketLatest) -> MapAction>;
pub type PacketMap = HashMap<PacketLatestKind, PacketMapFn>;

pub fn process_raw_packet<'a>(
    map: &'a PacketMap,
    raw_packet: RawPacketLatest<'a>,
) -> MapAction<'a> {
    return match map.get(&raw_packet.kind()) {
        Some(entry) => entry(raw_packet),
        None => MapAction::Relay::<'a>(raw_packet),
    };
}
