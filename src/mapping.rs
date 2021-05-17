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

use crate::state::{
    SplinterClient,
    SplinterState,
};

/// The function type for packet mapping
pub type PacketMapFn =
    Box<dyn Sync + Send + Fn(&SplinterClient, &SplinterState, &RawPacketLatest) -> bool>;

/// Packet map type
pub type PacketMap = HashMap<PacketLatestKind, PacketMapFn>;
