use std::{
    convert::TryFrom,
    sync::{atomic::Ordering, Arc},
};

use super::RelayPass;
use crate::protocol::current::{
    proto::{Packet756 as PacketLatest, Packet756Kind as PacketLatestKind},
    types::Vec3,
};

inventory::submit! {
    RelayPass(Box::new(|_proxy, _connection, client, _sender, lazy_packet, _destination| {
        if matches!(lazy_packet.kind(),
            PacketLatestKind::PlayServerHeldItemChange
            | PacketLatestKind::PlayClientHeldItemChange
            | PacketLatestKind::PlayServerPluginMessage
            ) {
            match lazy_packet.packet() {
                Ok(PacketLatest::PlayServerHeldItemChange(body)) => {
                    client.held_slot.store(body.slot, Ordering::Relaxed);
                },
                Ok(PacketLatest::PlayClientHeldItemChange(body)) => {
                    client.held_slot.store(body.slot as i8, Ordering::Relaxed);
                },
                Ok(PacketLatest::PlayServerPluginMessage(body)) => {
                    if body.channel == "splinter:splinter" {
                        match body.data.data[0] {
                            0 => {
                                if body.data.data.len() == 1+8+8+8 {
                                    let x = f64::from_be_bytes(TryFrom::try_from(&body.data.data[1..9]).unwrap());
                                    let y = f64::from_be_bytes(TryFrom::try_from(&body.data.data[9..17]).unwrap());
                                    let z = f64::from_be_bytes(TryFrom::try_from(&body.data.data[17..]).unwrap());
                                    let pos = Vec3 { x, y, z };
                                    // debug!("got position: {:?}", &pos);
                                    client.position.store(Arc::new(pos));
                                }
                            },
                            _ => {},
                        }
                    }
                },
                Ok(_) => unreachable!(),
                Err(e) => error!("Failed to deserialize held item message: {}", e),
            }
        }
    }))
}
