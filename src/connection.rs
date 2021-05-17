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
    mapping::{
        process_raw_packet,
        MapAction,
        PacketMap,
    },
    state::SplinterState,
};

pub struct SplinterClientConnection {
    pub craft_conn: CraftTcpConnection,
    pub sock_addr: SocketAddr,
}

pub struct SplinterServerConnection {
    pub craft_conn: CraftTcpConnection,
    pub sock_addr: SocketAddr,
}

pub trait HasCraftConn {
    fn craft_conn(&mut self) -> &mut CraftTcpConnection;
    fn sock_addr(&self) -> SocketAddr;

    fn write_packet(&mut self, packet: PacketLatest) {
        match self.craft_conn().write_packet(packet) {
            Err(e) => return error!("Failed to write packet to {}: {}", self.sock_addr(), e),
            Ok(_) => {}
        }
    }

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

pub enum EitherPacket {
    Normal(PacketLatest),
    Raw(Id, Vec<u8>),
}

pub fn handle_reader(
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
            Ok(Some(raw_packet)) => match process_raw_packet(&*packet_map, raw_packet) {
                MapAction::Relay(raw_packet) => {
                    if let Err(_) = writer_sender.send(EitherPacket::Raw(
                        raw_packet.id(),
                        raw_packet.data().to_owned(),
                    )) {
                        break;
                    }
                }
                MapAction::Server(packet) => {
                    if let Err(_) = server_writer_sender.send(EitherPacket::Normal(packet)) {
                        break;
                    }
                }
                MapAction::Client(packet) => {
                    if let Err(_) = client_writer_sender.send(EitherPacket::Normal(packet)) {
                        break;
                    }
                }
                MapAction::None => {}
            },
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
