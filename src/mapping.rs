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
    SplinterServerConnection,
    SplinterState,
};

/// Function for client-proxy packet mapping
pub type ClientPacketMapFn =
    Box<dyn Sync + Send + Fn(&Arc<SplinterClient>, &Arc<SplinterState>, &RawPacketLatest) -> bool>;

/// Function for proxy-server packet mapping
pub type ServerPacketMapFn = Box<
    dyn Sync
        + Send
        + Fn(
            &Arc<SplinterClient>,
            &Arc<SplinterServerConnection>,
            &Arc<SplinterState>,
            &RawPacketLatest,
        ) -> bool,
>;

/// Packet map type
pub type PacketMap<T> = HashMap<PacketLatestKind, T>;
