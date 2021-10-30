use std::{
    net::SocketAddr,
    sync::Arc,
};

use anyhow::Context;
use craftio_rs::{
    CraftAsyncReader,
    CraftAsyncWriter,
    CraftIo,
};

use super::{
    AsyncCraftConnection,
    AsyncCraftReader,
    AsyncCraftWriter,
};
use crate::{
    client::SplinterClient,
    current::{
        proto::{
            Packet756 as PacketLatest,
            PlayDisconnectSpec,
            PlayServerKeepAliveSpec,
            RawPacket756 as RawPacketLatest,
            StatusPongSpec,
            StatusRequestSpec,
            StatusResponseSpec,
        },
        protocol::{
            PacketDirection,
            State,
        },
        types::Chat,
    },
    events::LazyDeserializedPacket,
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
mod keepalive;
mod login;
mod sync;
mod tags;
mod uuid;
pub use chat::*;
pub use eid::*;
pub use login::*;
pub use sync::*;
pub use tags::*;
pub use uuid::*;

pub async fn handle_client_status(
    mut conn: AsyncCraftConnection,
    addr: SocketAddr,
    proxy: Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    conn.set_state(State::Status);
    conn.write_packet_async(PacketLatest::StatusResponse(StatusResponseSpec {
        response: proxy.config.server_status(&*proxy),
    }))
    .await?;
    loop {
        match conn.read_packet_async::<RawPacketLatest>().await? {
            Some(PacketLatest::StatusPing(body)) => {
                conn.write_packet_async(PacketLatest::StatusPong(StatusPongSpec {
                    payload: body.payload,
                }))
                .await?;
                break;
            }
            Some(PacketLatest::StatusRequest(StatusRequestSpec)) => {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketDestination {
    None,
    Server(u64),
    AllServers,
    Client,
}

type RelayPassFn = Box<
    dyn Send
        + Sync
        + Fn(
            &Arc<SplinterProxy>,
            &Arc<SplinterServerConnection>,
            &Arc<SplinterClient>,
            &PacketDirection,
            &mut LazyDeserializedPacket,
            &mut PacketDestination,
        ),
>;
pub struct RelayPass(pub RelayPassFn);

inventory::collect!(RelayPass);

pub async fn handle_server_packet(
    proxy: &Arc<SplinterProxy>,
    client: &Arc<SplinterClient>,
    reader: &mut AsyncCraftReader,
    server: &Arc<SplinterServer>,
    sender: &PacketDirection,
) -> anyhow::Result<Option<()>> {
    let packet_opt = reader
        .read_raw_packet_async::<RawPacketLatest>()
        .await
        .with_context(|| format!("Failed to read packet {}: ", server.id))?;
    match packet_opt {
        Some(raw_packet) => {
            let mut lazy_packet = LazyDeserializedPacket::from_raw_packet(raw_packet);
            let mut destination = PacketDestination::Client;
            for pass in inventory::iter::<RelayPass> {
                (pass.0)(
                    proxy,
                    &*client.active_server.load(),
                    client,
                    sender,
                    &mut lazy_packet,
                    &mut destination,
                );
            }
            let kind = lazy_packet.kind();
            send_packet(client, &destination, lazy_packet)
                .await
                .with_context(|| {
                    format!(
                        "Sending packet kind {:?} for client {} to destination {:?} failure",
                        kind, &client.name, destination
                    )
                })?;
            Ok(Some(()))
        }
        None => Ok(None),
    }
}

pub async fn handle_client_packet(
    proxy: &Arc<SplinterProxy>,
    client: &Arc<SplinterClient>,
    reader: &mut AsyncCraftReader,
    sender: &PacketDirection,
) -> anyhow::Result<Option<()>> {
    let packet_opt = reader
        .read_raw_packet_async::<RawPacketLatest>()
        .await
        .with_context(|| format!("Failed to read packet from {}", client.name))?;
    match packet_opt {
        Some(raw_packet) => {
            let mut lazy_packet = LazyDeserializedPacket::from_raw_packet(raw_packet);
            let mut destination = PacketDestination::AllServers;
            for pass in inventory::iter::<RelayPass> {
                (pass.0)(
                    proxy,
                    &*client.active_server.load(),
                    client,
                    sender,
                    &mut lazy_packet,
                    &mut destination,
                );
            }
            send_packet(client, &destination, lazy_packet)
                .await
                .with_context(|| {
                    format!("Sending packet from client \"{}\" failure", &client.name)
                })?;
            Ok(Some(()))
        }
        None => Ok(None),
    }
}

async fn send_packet<'a>(
    client: &Arc<SplinterClient>,
    destination: &PacketDestination,
    lazy_packet: LazyDeserializedPacket<'a>,
) -> anyhow::Result<()> {
    match destination {
        PacketDestination::Client => {
            write_packet(&mut *client.writer.lock().await, lazy_packet)
                .await
                .with_context(|| {
                    format!("Failed to write packet to client \"{}\"", &client.name,)
                })?;
        }
        PacketDestination::Server(server_id) => {
            let active_server = client.active_server.load();
            let dummy_servers = client.dummy_servers.load();
            let writer = &mut *(if active_server.server.id == *server_id {
                active_server.writer.lock().await
            } else {
                if let Some((_id, server_conn)) =
                    dummy_servers.iter().find(|(id, _)| *id == *server_id)
                {
                    server_conn.writer.lock().await
                } else {
                    bail!("No connected server from mapped server id");
                }
            });
            write_packet(writer, lazy_packet)
                .await
                .with_context(|| format!("Failed to write packet to server \"{}\"", server_id))?;
        }
        PacketDestination::AllServers => {
            for (server_id, server_conn) in client.dummy_servers.load().iter() {
                let writer = &mut *server_conn.writer.lock().await;
                write_packet(writer, lazy_packet.clone())
                    .await
                    .with_context(|| {
                        format!("Failed to write packet to server \"{}\"", server_id)
                    })?;
            }
            let active_server = client.active_server.load();
            let writer = &mut *active_server.writer.lock().await;

            write_packet(writer, lazy_packet).await.with_context(|| {
                format!(
                    "Failed to write packet to server \"{}\"",
                    active_server.server.id
                )
            })?;
        }
        PacketDestination::None => {}
    };
    Ok(())
}

async fn write_packet(
    writer: &mut AsyncCraftWriter,
    lazy_packet: LazyDeserializedPacket<'_>,
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
    pub async fn write_packet(&self, packet: LazyDeserializedPacket<'_>) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().await;
        if packet.is_deserialized() {
            writer.write_packet_async(packet.into_packet()?)
        } else {
            writer.write_raw_packet_async(packet.into_raw_packet().unwrap())
        }
        .await?;
        Ok(())
    }
    pub async fn send_kick(&self, reason: ClientKickReason) -> anyhow::Result<()> {
        self.write_packet(LazyDeserializedPacket::from_packet(
            PacketLatest::PlayDisconnect(PlayDisconnectSpec {
                reason: Chat::from_text(&reason.text()),
            }),
        ))
        .await
    }
    pub async fn send_keep_alive(&self, time: u128) -> anyhow::Result<()> {
        self.write_packet(LazyDeserializedPacket::from_packet(
            PacketLatest::PlayServerKeepAlive(PlayServerKeepAliveSpec {
                id: time as i64,
            }),
        ))
        .await
    }
}
