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
/// `is_alive` is a [`Arc`]<[`RwLock`]<[`bool`]>>. The as long as `is_alive` is true, then the reader
/// will continue reading. The reader can also turn off `is_alive` itself.
///
/// `packet_map` is an [`Arc`]<[`PacketMap`]> so that it can correctly determine what to do with certain packets.
/// `writer_sender`, `server_writer_sender`, and `client_writer_sender` are [`Sender`]<[`EitherPacket`]>.
/// `writer_sender` is the sender to wherever the original packet was going, for example if the
/// packet were original client bound, then `writer_sender` would be a clone of
/// `client_writer_sender`.
///
/// `client_name` is the user name of the client.
pub fn handle_reader(
    client: Arc<SplinterClient>,
    state: Arc<SplinterState>,
    is_alive: Arc<RwLock<bool>>,
    mut reader: impl CraftSyncReader,
    packet_map: Arc<PacketMap>,
    writer_sender: Sender<EitherPacket>,
    server_writer_sender: Sender<EitherPacket>,
    client_writer_sender: Sender<EitherPacket>,
    client_name: String,
) {
    while *is_alive.read().unwrap() {
        match reader.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(raw_packet)) => {
                if match packet_map.get(&raw_packet.kind()) {
                    Some(entry) => entry(&*client, &*state, &raw_packet),
                    None => true,
                } {
                    if let Err(e) = writer_sender.send(EitherPacket::Raw(
                        raw_packet.id(),
                        raw_packet.data().to_owned(),
                    )) {
                        error!("failed to send packet: {}", e);
                    }
                }
            }
            Ok(None) => {
                trace!("One connection closed for {}", client_name);
                break;
            }
            Err(e) => {
                error!("Failed to read packet for {}: {}", client_name, e);
            }
        }
    }
    *is_alive.write().unwrap() = false;
    trace!("reader thread closed for {}", client_name);
}

/// Handles reading a connection and deciding what to do with data
///
/// `is_alive` is a [`Arc`]<[`RwLock`]<[`bool`]>>. The as long as `is_alive` is true, then the reader
/// will continue reading. The reader can also turn off `is_alive` itself.
///
/// `client_name` is the user name of the client.
///
/// `writer_receiver` is a [`Receiver`]<[`EitherPacket`]> to receive any packets that are sent to this writer.
pub fn handle_writer(
    state: Arc<SplinterState>,
    is_alive: Arc<RwLock<bool>>,
    client_name: String,
    writer_receiver: Receiver<EitherPacket>,
    mut writer: impl CraftSyncWriter,
) {
    let mut recv = writer_receiver.iter();
    while *is_alive.read().unwrap() {
        match recv.next() {
            Some(packet) => {
                if let Err(e) = match packet {
                    EitherPacket::Normal(packet) => writer.write_packet(packet),
                    EitherPacket::Raw(id, data) => {
                        match RawPacketLatest::create(id, data.as_slice()) {
                            Ok(packet) => writer.write_raw_packet(packet),
                            Err(_e) => continue,
                        }
                    }
                } {
                    error!("Failed to send packet for {}: {}", client_name, e);
                }
            }
            None => break,
        }
    }
    *is_alive.write().unwrap() = false;
    trace!("writer thread closed for {}", client_name);
}
