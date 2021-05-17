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

/// Data associated with a connection between the proxy and a client
pub struct SplinterClientConnection {
    /// Connection to the client
    pub craft_conn: CraftTcpConnection,
    /// Address of the client
    pub sock_addr: SocketAddr,
}

/// Data associated with a connection between the proxy and a server
pub struct SplinterServerConnection {
    /// Conection to the server
    pub craft_conn: CraftTcpConnection,
    /// Address of the server
    pub sock_addr: SocketAddr,
}

/// Common types that can be written to with the MC protocol
pub trait HasCraftConn {
    /// Gets a mutable reference the connection
    fn craft_conn(&mut self) -> &mut CraftTcpConnection;
    /// Gets the address
    fn sock_addr(&self) -> SocketAddr;

    /// Writes a packet to the connection
    fn write_packet(&mut self, packet: PacketLatest) {
        match self.craft_conn().write_packet(packet) {
            Err(e) => return error!("Failed to write packet to {}: {}", self.sock_addr(), e),
            Ok(_) => {}
        }
    }

    /// Writes a raw packet to the connection
    fn write_raw_packet(&mut self, packet: RawPacketLatest) {
        match self.craft_conn().write_raw_packet(packet) {
            Err(e) => return error!("Failed to write packet to {}: {}", self.sock_addr(), e),
            Ok(_) => {}
        }
    }
}

impl HasCraftConn for SplinterClientConnection {
    fn craft_conn(&mut self) -> &mut CraftTcpConnection {
        &mut self.craft_conn
    }

    fn sock_addr(&self) -> SocketAddr {
        self.sock_addr
    }
}

impl HasCraftConn for SplinterServerConnection {
    fn craft_conn(&mut self) -> &mut CraftTcpConnection {
        &mut self.craft_conn
    }

    fn sock_addr(&self) -> SocketAddr {
        self.sock_addr
    }
}

/// A packet in the form of either a raw byte array or an unserialized packet
pub enum EitherPacket {
    /// An unserialized packet
    Normal(PacketLatest),
    /// A packet id with a byte array
    Raw(Id, Vec<u8>),
}

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
                        PacketDirection::ServerBound => client.servers.read().unwrap()[0]
                            .writer
                            .lock()
                            .unwrap()
                            .write_raw_packet(raw_packet),
                    } {
                        error!("Failed to send packet for {}: {:?}", client.name, direction);
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
