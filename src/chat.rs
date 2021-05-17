use mcproto_rs::{
    protocol::RawPacket,
    v1_16_3::{
        Packet753 as PacketLatest,
        Packet753Kind as PacketLatestKind,
        RawPacket753 as RawPacketLatest,
    },
};

use crate::mapping::{
    MapAction,
    PacketMap,
};

pub fn init(map: &mut PacketMap) {
    map.insert(
        PacketLatestKind::PlayClientChatMessage,
        Box::new(|state, raw_packet| {
            let packet = match raw_packet.deserialize() {
                Ok(packet) => packet,
                Err(e) => {
                    error!("failed to deserialize chat message from player: {}", e);
                    return MapAction::None;
                }
            };
            if let PacketLatest::PlayClientChatMessage(data) = packet {
                info!("got chat message: {}", data.message);
            }
            MapAction::None
        }),
    );
}
