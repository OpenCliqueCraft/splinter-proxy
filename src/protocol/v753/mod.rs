use std::{
    net::SocketAddr,
    sync::Arc,
};

use craftio_rs::{
    CraftAsyncReader,
    CraftAsyncWriter,
    CraftIo,
};
use mcproto_rs::{
    protocol::{
        HasPacketKind,
        State,
    },
    v1_16_3::{
        ChatPosition,
        HandshakeNextState,
        Packet753,
        PlayServerChatMessageSpec,
        RawPacket753,
        StatusPongSpec,
        StatusRequestSpec,
        StatusResponseSpec,
    },
};
use smol::lock::Mutex;

use super::{
    version::V753,
    AsyncCraftConnection,
    AsyncCraftReader,
    PacketSender,
};
use crate::{
    chat::ToChat,
    client::SplinterClient,
    commands::CommandSender,
    events::LazyDeserializedPacket,
    mapping::SplinterMapping,
    protocol::{
        version,
        ProtocolVersion,
    },
    proxy::SplinterProxy,
    server::SplinterServerConnection,
};

mod eid;
mod login;
mod tags;
mod uuid;
pub use eid::*;
pub use login::*;
pub use tags::*;
pub use uuid::*;

pub async fn handle_client_status(
    mut conn: AsyncCraftConnection,
    addr: SocketAddr,
    proxy: Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    conn.set_state(State::Status);
    conn.write_packet_async(Packet753::StatusResponse(StatusResponseSpec {
        response: proxy.config.server_status(&*proxy),
    }))
    .await?;
    loop {
        match conn.read_packet_async::<RawPacket753>().await? {
            Some(Packet753::StatusPing(body)) => {
                conn.write_packet_async(Packet753::StatusPong(StatusPongSpec {
                    payload: body.payload,
                }))
                .await?;
                break;
            }
            Some(Packet753::StatusRequest(StatusRequestSpec)) => {
                // do nothing.
                // notchian client does not like it when we respond
                // with a server status to this message
            }
            Some(other) => error!("Unexpected packet {:?} from {}", other, addr),
            None => break,
        }
    }
    Ok(())
}

pub struct RelayPassFn(
    pub  Box<
        dyn Send
            + Sync
            + Fn(
                &Arc<SplinterProxy>,
                PacketSender,
                &mut LazyDeserializedPacket<V753>,
                &mut SplinterMapping,
            ) -> Option<u64>,
    >,
);

inventory::collect!(RelayPassFn);

pub async fn handle_server_relay(
    proxy: Arc<SplinterProxy>,
    client: Arc<SplinterClient<V753>>,
    server_conn: Arc<Mutex<SplinterServerConnection>>,
    mut server_reader: AsyncCraftReader,
) -> anyhow::Result<()> {
    let server = Arc::clone(&server_conn.lock().await.server);
    loop {
        // server->proxy->client
        if !*client.alive.lock().await || !server_conn.lock().await.alive {
            break;
        }
        let packet_opt = match server_reader.read_raw_packet_async::<RawPacket753>().await {
            Ok(packet) => packet,
            Err(e) => {
                error!("Failed to read packet from server {}: {}", server.id, e);
                continue;
            }
        };
        match packet_opt {
            Some(raw_packet) => {
                let mut lazy_packet =
                    LazyDeserializedPacket::<version::V753>::from_raw_packet(raw_packet);
                let map = &mut *proxy.mapping.lock().await;
                for pass in inventory::iter::<RelayPassFn> {
                    (pass.0)(&proxy, PacketSender::Server(&server), &mut lazy_packet, map);
                }
                let writer = &mut *client.writer.lock().await;
                if lazy_packet.is_deserialized() {
                    if let Err(e) = writer
                        .write_packet_async(match lazy_packet.into_packet() {
                            Ok(packet) => packet,
                            Err(e) => {
                                error!("Failed to parse packet from server {}: {}", server.id, e);
                                continue;
                            }
                        })
                        .await
                    {
                        error!(
                            "Failed to relay modified packet from server {} to client {}: {}",
                            server.id, &client.name, e
                        );
                        continue;
                    }
                } else {
                    if let Err(e) = writer
                        .write_raw_packet_async(match lazy_packet.into_raw_packet() {
                            Some(packet) => packet,
                            None => {
                                error!("Failed to get raw packet when there is none {}", server.id);
                                continue;
                            }
                        })
                        .await
                    {
                        error!(
                            "Failed to relay raw packet from server {} to client {}: {}",
                            server.id, &client.name, e
                        );
                        continue;
                    }
                }
            }
            None => {
                // connection closed
                break;
            }
        }
    }
    server_conn.lock().await.alive = false;
    info!(
        "Server connection between {} and server id {} closed",
        client.name, server.id
    );
    Ok(())
}

pub async fn handle_client_relay(
    proxy: Arc<SplinterProxy>,
    client: Arc<SplinterClient<V753>>,
    mut client_reader: AsyncCraftReader,
    client_addr: SocketAddr,
) -> anyhow::Result<()> {
    loop {
        // client->proxy->server
        if !*client.alive.lock().await {
            break;
        }
        let packet_opt = match client_reader.read_raw_packet_async::<RawPacket753>().await {
            Ok(packet) => packet,
            Err(e) => {
                error!(
                    "Failed to read packet from {}, {}: {}",
                    client.name, client_addr, e
                );
                continue;
            }
        };
        match packet_opt {
            Some(raw_packet) => {
                let mut lazy_packet =
                    LazyDeserializedPacket::<version::V753>::from_raw_packet(raw_packet);
                let map = &mut *proxy.mapping.lock().await;
                for pass in inventory::iter::<RelayPassFn> {
                    (pass.0)(&proxy, PacketSender::Proxy, &mut lazy_packet, map);
                }
                let mut server = client.servers.lock().await;
                let writer = &mut server
                    .get_mut(&client.active_server_id)
                    .unwrap()
                    .lock()
                    .await
                    .writer; // TODO: take a look at this double lock
                if lazy_packet.is_deserialized() {
                    let kind = lazy_packet.kind();
                    if let Err(e) = writer
                        .write_packet_async(match lazy_packet.into_packet() {
                            Ok(packet) => packet,
                            Err(e) => {
                                error!(
                                    "Failed to parse packet {:?} from {}, {}: {}",
                                    kind, &client.name, client_addr, e
                                );
                                continue;
                            }
                        })
                        .await
                    {
                        error!(
                            "Failed to relay modified packet from {}, {} to server id {}: {}",
                            &client.name, client_addr, client.active_server_id, e
                        );
                        continue;
                    }
                } else {
                    if let Err(e) = writer
                        .write_raw_packet_async(match lazy_packet.into_raw_packet() {
                            Some(packet) => packet,
                            None => {
                                error!(
                                    "Failed to get raw packet when there is none {}, {}",
                                    &client.name, client_addr
                                );
                                continue;
                            }
                        })
                        .await
                    {
                        error!(
                            "Failed to relay raw packet from {}, {} to server id {}: {}",
                            &client.name, client_addr, client.active_server_id, e
                        );
                        continue;
                    }
                }
            }
            None => {
                // connection closed
                break;
            }
        }
    }
    proxy.players.write().unwrap().remove(&client.name);
    *client.alive.lock().await = false;
    info!(
        "Client \"{}\", {} connection closed",
        client.name, client_addr
    );
    Ok(())
}

impl SplinterClient<V753> {
    pub async fn write_packet<'a>(
        &self,
        packet: LazyDeserializedPacket<'a, V753>,
    ) -> anyhow::Result<()> {
        if packet.is_deserialized() {
            self.writer
                .lock()
                .await
                .write_packet_async(
                    packet
                        .into_packet()
                        .map_err(|e| anyhow!(format!("{}", e)))?,
                )
                .await
                .map_err(|e| anyhow!(format!("{}", e)))
        } else {
            self.writer
                .lock()
                .await
                .write_raw_packet_async(packet.into_raw_packet().unwrap())
                .await
                .map_err(|e| anyhow!(e))
        }
    }
    pub async fn send_message(
        &self,
        msg: impl ToChat,
        sender: &CommandSender,
    ) -> anyhow::Result<()> {
        self.write_packet(LazyDeserializedPacket::<V753>::from_packet(
            Packet753::PlayServerChatMessage(PlayServerChatMessageSpec {
                message: msg.to_chat(),
                position: match sender {
                    CommandSender::Player(_) => ChatPosition::ChatBox,
                    CommandSender::Console => ChatPosition::SystemMessage,
                },
                sender: sender.uuid(),
            }),
        ))
        .await
    }
}
