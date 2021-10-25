use std::sync::Arc;

use super::RelayPass;
use crate::current::proto::{
    Packet756 as PacketLatest,
    Packet756Kind as PacketLatestKind,
};

inventory::submit! {
    RelayPass(Box::new(|_proxy, _connection, client, _sender, lazy_packet, _destination| {
        if lazy_packet.kind() == PacketLatestKind::PlayServerHeldItemChange || lazy_packet.kind() == PacketLatestKind::PlayClientHeldItemChange {
            match lazy_packet.packet() {
                Ok(PacketLatest::PlayServerHeldItemChange(body)) => {
                    client.held_slot.store(Arc::new(body.slot));
                },
                Ok(PacketLatest::PlayClientHeldItemChange(body)) => {
                    client.held_slot.store(Arc::new(body.slot as i8));
                },
                Ok(_) => unreachable!(),
                Err(e) => error!("Failed to deserialize held item message: {}", e),
            }
        }
    }))
}
