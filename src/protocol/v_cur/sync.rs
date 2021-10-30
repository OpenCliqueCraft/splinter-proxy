use std::sync::atomic::Ordering;

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
                    client.held_slot.store(body.slot, Ordering::Relaxed);
                },
                Ok(PacketLatest::PlayClientHeldItemChange(body)) => {
                    client.held_slot.store(body.slot as i8, Ordering::Relaxed);
                },
                Ok(_) => unreachable!(),
                Err(e) => error!("Failed to deserialize held item message: {}", e),
            }
        }
    }))
}
