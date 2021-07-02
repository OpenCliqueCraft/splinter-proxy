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
    types::Chat,
    v1_16_3::{
        ChatPosition,
        HandshakeNextState,
        Packet753,
        PlayDisconnectSpec,
        PlayServerChatMessageSpec,
        PlayServerKeepAliveSpec,
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
    AsyncCraftWriter,
    PacketDestination,
    PacketSender,
};
use crate::{
    chat::ToChat,
    client::{
        ClientVersion,
        SplinterClient,
    },
    commands::CommandSender,
    events::LazyDeserializedPacket,
    mapping::SplinterMapping,
    protocol::{
        version,
        ProtocolVersion,
    },
    proxy::{
        ClientKickReason,
        SplinterProxy,
    },
    server::{
        SplinterServer,
        SplinterServerConnection,
    },
};

mod chat;
mod eid;
mod login;
mod tags;
mod uuid;
pub use chat::*;
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

type RelayPassFn = Box<
    dyn Send
        + Sync
        + Fn(
            &Arc<SplinterProxy>,
            &PacketSender,
            &mut LazyDeserializedPacket<V753>,
            &mut SplinterMapping,
            &mut PacketDestination,
        ),
>;
pub struct RelayPass(pub RelayPassFn);

inventory::collect!(RelayPass);

pub async fn handle_server_packet<'a>(
    proxy: &Arc<SplinterProxy>,
    client: &Arc<SplinterClient>,
    reader: &mut AsyncCraftReader,
    server: &Arc<SplinterServer>,
    mut destination: PacketDestination,
    sender: &PacketSender<'a>,
) -> anyhow::Result<Option<()>> {
    let packet_opt = match reader.read_raw_packet_async::<RawPacket753>().await {
        Ok(packet) => packet,
        Err(e) => {
            bail!("Failed to read packet {}: {}", server.id, e);
        }
    };
    match packet_opt {
        Some(raw_packet) => {
            let mut lazy_packet =
                LazyDeserializedPacket::<version::V753>::from_raw_packet(raw_packet);
            let map = &mut *proxy.mapping.lock().await;
            for pass in inventory::iter::<RelayPass> {
                (pass.0)(&proxy, &sender, &mut lazy_packet, map, &mut destination);
            }
            if let Err(e) = send_packet(&client, &destination, lazy_packet).await {
                bail!("Sending packet {} failure: {}", server.id, e);
            }
            Ok(Some(()))
        }
        None => Ok(None),
    }
}

pub async fn handle_client_packet<'a>(
    proxy: &Arc<SplinterProxy>,
    client: &Arc<SplinterClient>,
    reader: &mut AsyncCraftReader,
    mut destination: PacketDestination,
    sender: &PacketSender<'a>,
) -> anyhow::Result<Option<()>> {
    let packet_opt = match reader.read_raw_packet_async::<RawPacket753>().await {
        Ok(packet) => packet,
        Err(e) => {
            bail!("Failed to read packet from {}: {}", client.name, e);
        }
    };
    match packet_opt {
        Some(raw_packet) => {
            let mut lazy_packet =
                LazyDeserializedPacket::<version::V753>::from_raw_packet(raw_packet);
            let map = &mut *proxy.mapping.lock().await;
            for pass in inventory::iter::<RelayPass> {
                (pass.0)(&proxy, &sender, &mut lazy_packet, map, &mut destination);
            }
            if let Err(e) = send_packet(&client, &destination, lazy_packet).await {
                bail!(
                    "Sending packet from client \"{}\" failure: {}",
                    &client.name,
                    e
                );
            }
            Ok(Some(()))
        }
        None => Ok(None),
    }
}

async fn send_packet(
    client: &Arc<SplinterClient>,
    destination: &PacketDestination,
    lazy_packet: LazyDeserializedPacket<'_, V753>,
) -> anyhow::Result<()> {
    match destination {
        PacketDestination::Client => {
            let writer = &mut *client.writer.lock().await;
            if let Err(e) = write_packet(writer, lazy_packet).await {
                bail!(
                    "Failed to write packet to client \"{}\": {}",
                    &client.name,
                    e
                );
            }
        }
        PacketDestination::Server(server_id) => {
            let servers = client.servers.lock().await;
            let mut server_conn = servers
                .get(server_id)
                .ok_or_else(|| {
                    anyhow!(
                        "Tried to redirect packet to unknown server id \"{}\"",
                        server_id
                    )
                })?
                .lock()
                .await;
            let writer = &mut server_conn.writer;
            if let Err(e) = write_packet(writer, lazy_packet).await {
                bail!(
                    "Failed to write packet to server \"{}\": {}",
                    server_conn.server.id,
                    e
                );
            }
        }
        PacketDestination::None => return Ok(()),
    };
    Ok(())
}

async fn write_packet(
    writer: &mut AsyncCraftWriter,
    lazy_packet: LazyDeserializedPacket<'_, V753>,
) -> anyhow::Result<()> {
    if lazy_packet.is_deserialized() {
        writer
            .write_packet_async(lazy_packet.into_packet()?)
            .await?;
    } else {
        writer
            .write_raw_packet_async(lazy_packet.into_raw_packet().unwrap())
            .await?;
    }
    Ok(())
}

impl SplinterClient {
    pub async fn write_packet_v753(
        &self,
        packet: LazyDeserializedPacket<'_, V753>,
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
    pub async fn send_kick_v753(&self, reason: ClientKickReason) -> anyhow::Result<()> {
        self.write_packet_v753(LazyDeserializedPacket::<V753>::from_packet(
            Packet753::PlayDisconnect(PlayDisconnectSpec {
                reason: Chat::from_text(&reason.text()),
            }),
        ))
        .await
    }
    pub async fn send_keep_alive_v753(&self, time: u128) -> anyhow::Result<()> {
        self.write_packet_v753(LazyDeserializedPacket::<V753>::from_packet(
            Packet753::PlayServerKeepAlive(PlayServerKeepAliveSpec {
                id: time as i64,
            }),
        ))
        .await
    }
}
