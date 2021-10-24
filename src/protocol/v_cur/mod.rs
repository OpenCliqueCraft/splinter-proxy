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

type RelayPassFn = Box<
    dyn Send
        + Sync
        + Fn(
            &Arc<SplinterProxy>,
            &Arc<SplinterServerConnection>,
            &Arc<SplinterClient>,
            &PacketDirection,
            &mut LazyDeserializedPacket,
            &mut Option<PacketDirection>,
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
            let mut destination = Some(*sender);
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
            if let Some(dir) = destination {
                send_packet(client, &dir, lazy_packet)
                    .await
                    .with_context(|| format!("Sending packet {} failure", server.id))?;
            }
            Ok(Some(()))
        }
        None => Ok(None),
    }
}

// uh yeah, TODO: this is the same function as above minus like 2 lines
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
            let mut destination = Some(*sender);
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
            if let Some(dir) = destination {
                send_packet(client, &dir, lazy_packet)
                    .await
                    .with_context(|| {
                        format!("Sending packet from client \"{}\" failure", &client.name)
                    })?;
            }
            Ok(Some(()))
        }
        None => Ok(None),
    }
}

async fn send_packet(
    client: &Arc<SplinterClient>,
    destination: &PacketDirection,
    lazy_packet: LazyDeserializedPacket<'_>,
) -> anyhow::Result<()> {
    match destination {
        PacketDirection::ClientBound => {
            write_packet(&mut *client.writer.lock().await, lazy_packet)
                .await
                .with_context(|| {
                    format!("Failed to write packet to client \"{}\"", &client.name,)
                })?;
        }
        PacketDirection::ServerBound => {
            write_packet(
                &mut *client.active_server.load().writer.lock().await,
                lazy_packet,
            )
            .await
            .with_context(|| {
                format!(
                    "Failed to write packet to server \"{}\"",
                    client.server_id()
                )
            })?;
        }
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
