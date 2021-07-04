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
    protocol::State,
    types::Chat,
    v1_17_0::{
        Packet755,
        PlayDisconnectSpec,
        PlayServerKeepAliveSpec,
        RawPacket755,
        StatusPongSpec,
        StatusRequestSpec,
        StatusResponseSpec,
    },
};

use super::{
    version::V755,
    AsyncCraftConnection,
    AsyncCraftReader,
    AsyncCraftWriter,
    PacketDestination,
    PacketSender,
};
use crate::{
    client::SplinterClient,
    events::LazyDeserializedPacket,
    mapping::SplinterMapping,
    protocol::version,
    proxy::{
        ClientKickReason,
        SplinterProxy,
    },
    server::SplinterServer,
};

mod chat;
mod eid;
mod keepalive;
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
    conn.write_packet_async(Packet755::StatusResponse(StatusResponseSpec {
        response: proxy.config.server_status(&*proxy),
    }))
    .await?;
    loop {
        match conn.read_packet_async::<RawPacket755>().await? {
            Some(Packet755::StatusPing(body)) => {
                conn.write_packet_async(Packet755::StatusPong(StatusPongSpec {
                    payload: body.payload,
                }))
                .await?;
                break;
            }
            Some(Packet755::StatusRequest(StatusRequestSpec)) => {
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
            &mut LazyDeserializedPacket<V755>,
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
    let packet_opt = match reader.read_raw_packet_async::<RawPacket755>().await {
        Ok(packet) => packet,
        Err(e) => {
            bail!("Failed to read packet {}: {}", server.id, e);
        }
    };
    match packet_opt {
        Some(raw_packet) => {
            let mut lazy_packet =
                LazyDeserializedPacket::<version::V755>::from_raw_packet(raw_packet);
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
    let packet_opt = match reader.read_raw_packet_async::<RawPacket755>().await {
        Ok(packet) => packet,
        Err(e) => {
            bail!("Failed to read packet from {}: {}", client.name, e);
        }
    };
    match packet_opt {
        Some(raw_packet) => {
            let mut lazy_packet =
                LazyDeserializedPacket::<version::V755>::from_raw_packet(raw_packet);
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
    lazy_packet: LazyDeserializedPacket<'_, V755>,
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
    lazy_packet: LazyDeserializedPacket<'_, V755>,
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
    pub async fn write_packet_v755(
        &self,
        packet: LazyDeserializedPacket<'_, V755>,
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
    pub async fn send_kick_v755(&self, reason: ClientKickReason) -> anyhow::Result<()> {
        self.write_packet_v755(LazyDeserializedPacket::<V755>::from_packet(
            Packet755::PlayDisconnect(PlayDisconnectSpec {
                reason: Chat::from_text(&reason.text()),
            }),
        ))
        .await
    }
    pub async fn send_keep_alive_v755(&self, time: u128) -> anyhow::Result<()> {
        self.write_packet_v755(LazyDeserializedPacket::<V755>::from_packet(
            Packet755::PlayServerKeepAlive(PlayServerKeepAliveSpec {
                id: time as i64,
            }),
        ))
        .await
    }
}
