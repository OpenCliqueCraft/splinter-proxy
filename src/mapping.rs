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

pub type PacketMapFn =
    Box<dyn Sync + Send + Fn(&SplinterClient, &SplinterState, &RawPacketLatest) -> bool>;
pub type PacketMap = HashMap<PacketLatestKind, PacketMapFn>;
