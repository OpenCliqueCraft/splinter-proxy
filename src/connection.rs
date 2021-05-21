use std::{
    net::SocketAddr,
    sync::{
        mpsc::{
            Receiver,
            Sender,
        },
        Arc,
        RwLock,
    },
};

use craftio_rs::{
    CraftSyncReader,
    CraftSyncWriter,
    CraftTcpConnection,
};
use mcproto_rs::{
    protocol::{
        HasPacketId,
        HasPacketKind,
        Id,
        PacketDirection,
        RawPacket,
    },
    v1_16_3::{
        Packet753 as PacketLatest,
        RawPacket753 as RawPacketLatest,
    },
};

use crate::{
    config::SplinterProxyConfiguration,
    mapping::PacketMap,
    state::{
        SplinterClient,
        SplinterState,
    },
};

/// Handles reading a connection and deciding what to do with data
///
/// `client` contains the state of the client.
///
/// `state` is the state of the proxy.
///
/// `is_alive` is a [`Arc`]<[`RwLock`]<[`bool`]>>. The as long as `is_alive` is true, then the reader
/// will continue reading. The reader can also turn off `is_alive` itself.
///
/// `packet_map` is an [`Arc`]<[`PacketMap`]> so that it can correctly determine what to do with certain packets.
///
/// `direction` is the packet flow, whether packets are coming from server (client bound) or coming
/// from client (server bound)
pub fn handle_reader(
    client: Arc<SplinterClient>,
    state: Arc<SplinterState>,
    is_alive: Arc<RwLock<bool>>,
    mut reader: impl CraftSyncReader,
    packet_map: Arc<PacketMap>,
    direction: PacketDirection,
) {
    while *is_alive.read().unwrap() {
        match reader.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(raw_packet)) => {
                if match packet_map.get(&raw_packet.kind()) {
                    Some(entry) => entry(&*client, &*state, &raw_packet),
                    None => true,
                } {
                    if let Err(e) = match direction {
                        PacketDirection::ClientBound => {
                            client.writer.lock().unwrap().write_raw_packet(raw_packet)
                        }
                        PacketDirection::ServerBound => client
                            .servers
                            .read()
                            .unwrap()
                            .get(&0)
                            .unwrap()
                            .writer
                            .lock()
                            .unwrap()
                            .write_raw_packet(raw_packet),
                    } {
                        error!(
                            "Failed to send packet for {}: {:?}, {}",
                            client.name, direction, e
                        );
                    }
                }
            }
            Ok(None) => {
                break;
            }
            Err(e) => {
                error!("Failed to read packet for {}: {}", client.name, e);
            }
        }
    }
    *is_alive.write().unwrap() = false;
    trace!("reader thread closed for {}", client.name);
}
