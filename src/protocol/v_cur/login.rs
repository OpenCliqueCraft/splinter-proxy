use std::{
    collections::HashSet,
    str,
    sync::Arc,
};

use anyhow::Context;
use craftio_rs::{
    CraftAsyncReader,
    CraftAsyncWriter,
    CraftIo,
};

use crate::{
    protocol::{
        current::{
            proto::{
                ClientChatMode,
                ClientDisplayedSkinParts,
                ClientMainHand,
                ClientStatusAction,
                HandshakeNextState,
                HandshakeSpec,
                LoginSetCompressionSpec,
                LoginStartSpec,
                LoginSuccessSpec,
                PlayClientHeldItemChangeSpec,
                PlayClientPluginMessageSpec,
                PlayClientSettingsSpec,
                PlayClientStatusSpec,
                PlayServerPluginMessageSpec,
                PlayTagsSpec,
                PlayTeleportConfirmSpec,
            },
            protocol::PacketDirection,
            types::VarInt,
            uuid::UUID4,
            PacketLatest,
            RawPacketLatest,
        },
        plugin,
        AsyncCraftWriter,
        ClientBuilder,
        Tags,
    },
    proxy::{
        client::{
            ChatMode,
            ClientSettings,
            MainHand,
            SkinPart,
        },
        server::SplinterServerConnection,
        SplinterProxy,
    },
};

pub async fn handle_client_login_packet(
    next_sender: &mut PacketDirection,
    builder: &mut ClientBuilder<'_>,
    client_conn_reader: &mut (impl CraftAsyncReader + CraftIo + Send + Sync),
) -> anyhow::Result<Option<bool>> {
    let packet = match next_sender {
        PacketDirection::ServerBound => {
            client_conn_reader
                .read_packet_async::<RawPacketLatest>()
                .await?
        }
        PacketDirection::ClientBound => {
            builder
                .server_conn
                .as_mut()
                .unwrap()
                .reader
                .get_mut()
                .read_packet_async::<RawPacketLatest>()
                .await?
        }
    };
    if let Some(packet) = packet {
        match packet {
            PacketLatest::LoginStart(body) => {
                builder.login_start(&body.name).await?;
                *next_sender = PacketDirection::ClientBound;
            }
            PacketLatest::LoginSetCompression(body) => {
                builder.login_set_compression(*body.threshold);
                *next_sender = PacketDirection::ClientBound;
            }
            PacketLatest::LoginSuccess(body) => {
                builder.server_conn.as_mut().unwrap().uuid = body.uuid;
                // builder.proxy.mapping.lock().await.uuids.insert(
                // builder.uuid.unwrap(),
                // (builder.server_conn.as_ref().unwrap().server.id, body.uuid),
                // );
                // debug!(
                // "player uuid ({}, {}) -> {}",
                // builder.server_conn.as_ref().unwrap().server.id,
                // body.uuid,
                // builder.uuid.unwrap()
                // );
                // body.uuid = builder.uuid.unwrap(); // we're not relaying
                builder.login_success(client_conn_reader).await?;
                *next_sender = PacketDirection::ClientBound;
            }
            PacketLatest::PlayJoinGame(mut body) => {
                builder.server_conn.as_mut().unwrap().eid = body.entity_id;
                let map = &mut *builder.proxy.mapping.lock().await;
                let server_id = builder.server_conn.as_ref().unwrap().server.id;
                body.entity_id = if let Some(existing_id) =
                    map.eids.get_by_right(&(server_id, body.entity_id))
                {
                    *existing_id
                } else {
                    map.register_eid_mapping(server_id, body.entity_id)
                };
                debug!(
                    "player eid ({}, {}) -> {}",
                    server_id,
                    builder.server_conn.as_ref().unwrap().eid,
                    body.entity_id,
                );
                builder
                    .client_writer
                    .write_packet_async(PacketLatest::PlayJoinGame(body))
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to relay join game packet for \"{}\"",
                            builder.name.as_ref().unwrap()
                        )
                    })?;
                builder.play_join_game().await?;
                *next_sender = PacketDirection::ServerBound;
            }
            PacketLatest::PlayClientPluginMessage(_body) => {
                //..
                *next_sender = PacketDirection::ServerBound;
            }
            PacketLatest::PlayServerPluginMessage(body) => {
                // we need to register the plugin channel so that spigot thinks its okay to send
                // through it
                if body.channel == "minecraft:register" {
                    if let Ok("splinter:splinter") =
                        str::from_utf8(&body.data[0..body.data.len() - 1])
                    {
                        builder.server_conn.as_mut().unwrap().writer.get_mut().write_packet_async(PacketLatest::PlayClientPluginMessage(PlayClientPluginMessageSpec {
                            channel: "minecraft:register".into(),
                            data: ["splinter:splinter".as_bytes(), &[0]].concat().into(),
                        })).await.with_context(|| { format!("failed to respond to splinter plugin register message for \"{}\"", builder.name.as_ref().unwrap()) })?;
                    }
                }
                *next_sender = PacketDirection::ClientBound;
            }
            PacketLatest::PlayClientSettings(body) => {
                builder.play_client_settings(body.clone().into()).await?;
                *next_sender = PacketDirection::ClientBound;
            }
            packet @ (PacketLatest::PlayServerDifficulty(_)
            | PacketLatest::PlayServerPlayerAbilities(_)
            | PacketLatest::PlayDeclareRecipes(_)
            | PacketLatest::PlayServerHeldItemChange(_)) => {
                builder
                    .client_writer
                    .write_packet_async(packet)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to relay server packet to {}",
                            builder.name.as_ref().unwrap()
                        )
                    })?;
                *next_sender = PacketDirection::ClientBound;
            }
            PacketLatest::PlayTags(body) => {
                let tags = Tags::from(&body);
                builder.play_tags(tags).await?;
                return Ok(Some(true));
            }
            PacketLatest::LoginEncryptionRequest(_body) => {
                bail!("Server attempted to initiate encryption. Did you turn off online mode?");
            }
            _ => warn!(
                "Unexpected packet from {}: {:?}",
                builder.client_addr, packet
            ),
        }
        Ok(Some(false))
    } else {
        Ok(None)
    }
}

pub async fn send_handshake(
    server_conn: &mut SplinterServerConnection,
    proxy: &Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(PacketLatest::Handshake(HandshakeSpec {
            version: proxy.config.protocol.into(),
            server_address: format!("{}", server_conn.server.address.ip()),
            server_port: server_conn.server.address.port(),
            next_state: HandshakeNextState::Login,
        }))
        .await
        .map_err(|e| e.into())
}
pub async fn send_login_start(
    server_conn: &mut SplinterServerConnection,
    name: impl ToString,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(PacketLatest::LoginStart(LoginStartSpec {
            name: name.to_string(),
        }))
        .await
        .map_err(|e| e.into())
}
pub async fn send_set_compression(
    writer: &mut AsyncCraftWriter,
    threshold: i32,
) -> anyhow::Result<()> {
    writer
        .write_packet_async(PacketLatest::LoginSetCompression(LoginSetCompressionSpec {
            threshold: threshold.into(),
        }))
        .await
        .map_err(|e| e.into())
}
pub async fn send_login_success(
    writer: &mut AsyncCraftWriter,
    name: String,
    uuid: UUID4,
) -> anyhow::Result<()> {
    writer
        .write_packet_async(PacketLatest::LoginSuccess(LoginSuccessSpec {
            username: name,
            uuid,
        }))
        .await
        .map_err(|e| e.into())
}
pub async fn send_brand(
    writer: &mut AsyncCraftWriter,
    brand: impl AsRef<str>,
) -> anyhow::Result<()> {
    writer
        .write_packet_async(PacketLatest::PlayServerPluginMessage(
            PlayServerPluginMessageSpec {
                channel: "minecraft:brand".into(),
                data: [&[brand.as_ref().len() as u8], brand.as_ref().as_bytes()]
                    .concat()
                    .into(),
            },
        ))
        .await
        .map_err(|e| e.into())
}
pub async fn send_client_settings(
    server_conn: &mut SplinterServerConnection,
    settings: ClientSettings,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(PacketLatest::PlayClientSettings(settings.into()))
        .await
        .map_err(|e| e.into())
}
pub async fn send_tags(writer: &mut AsyncCraftWriter, tags: &Tags) -> anyhow::Result<()> {
    writer
        .write_packet_async(PacketLatest::PlayTags(PlayTagsSpec::from(tags)))
        .await
        .map_err(|e| e.into())
}

pub async fn send_teleport_confirm(
    server_conn: &mut SplinterServerConnection,
    teleport_id: VarInt,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(PacketLatest::PlayTeleportConfirm(PlayTeleportConfirmSpec {
            teleport_id: teleport_id.into(),
        }))
        .await
        .map_err(|e| e.into())
}

pub async fn send_client_status(
    server_conn: &mut SplinterServerConnection,
    status: ClientStatusAction,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(PacketLatest::PlayClientStatus(PlayClientStatusSpec {
            action: status,
        }))
        .await
        .map_err(|e| e.into())
}

pub async fn send_held_item_change(
    server_conn: &mut SplinterServerConnection,
    slot: i8,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(PacketLatest::PlayClientHeldItemChange(
            PlayClientHeldItemChangeSpec {
                slot: slot as i16,
            },
        ))
        .await
        .map_err(|e| e.into())
}

pub async fn send_position_set(
    writer: &mut AsyncCraftWriter,
    x: f64,
    y: f64,
    z: f64,
) -> anyhow::Result<()> {
    writer
        .write_packet_async(PacketLatest::PlayClientPluginMessage(
            PlayClientPluginMessageSpec {
                channel: "splinter:splinter".into(),
                data: plugin::position_set(x, y, z).into(),
            },
        ))
        .await
        .map_err(|e| anyhow!(e))
}

impl From<ClientChatMode> for ChatMode {
    fn from(mode: ClientChatMode) -> Self {
        match mode {
            ClientChatMode::Enabled => Self::Enabled,
            ClientChatMode::Hidden => Self::Hidden,
            ClientChatMode::CommandsOnly => Self::CommandsOnly,
        }
    }
}
impl From<ChatMode> for ClientChatMode {
    fn from(mode: ChatMode) -> ClientChatMode {
        match mode {
            ChatMode::Enabled => ClientChatMode::Enabled,
            ChatMode::Hidden => ClientChatMode::Hidden,
            ChatMode::CommandsOnly => ClientChatMode::CommandsOnly,
        }
    }
}

pub fn client_displayed_skin_parts_into_set(parts: ClientDisplayedSkinParts) -> HashSet<SkinPart> {
    let mut set = HashSet::new();
    if parts.is_cape_enabled() {
        set.insert(SkinPart::Cape);
    }
    if parts.is_jacket_enabled() {
        set.insert(SkinPart::Jacket);
    }
    if parts.is_left_sleeve_enabled() {
        set.insert(SkinPart::LeftSleeve);
    }
    if parts.is_right_sleeve_enabled() {
        set.insert(SkinPart::RightSleeve);
    }
    if parts.is_left_pants_leg_enabled() {
        set.insert(SkinPart::LeftPant);
    }
    if parts.is_right_pant_legs_enabled() {
        set.insert(SkinPart::RightPant);
    }
    if parts.is_hat_enabled() {
        set.insert(SkinPart::Hat);
    }
    set
}
pub fn set_into_client_displayed_skin_parts(set: HashSet<SkinPart>) -> ClientDisplayedSkinParts {
    let mut parts = ClientDisplayedSkinParts::default();
    parts.set_cape_enabled(set.contains(&SkinPart::Cape));
    parts.set_jacket_enabled(set.contains(&SkinPart::Jacket));
    parts.set_left_sleeve_enabled(set.contains(&SkinPart::LeftSleeve));
    parts.set_right_sleeve_enabled(set.contains(&SkinPart::RightSleeve));
    parts.set_left_pants_leg_enabled(set.contains(&SkinPart::LeftPant));
    parts.set_right_pant_legs_enabled(set.contains(&SkinPart::RightPant));
    parts.set_hat_enabled(set.contains(&SkinPart::Hat));
    parts
}

impl From<PlayClientSettingsSpec> for ClientSettings {
    fn from(settings: PlayClientSettingsSpec) -> Self {
        Self {
            locale: settings.locale,
            view_distance: settings.view_distance,
            chat_mode: settings.chat_mode.into(),
            chat_colors: settings.chat_colors,
            skin_parts: client_displayed_skin_parts_into_set(settings.displayed_skin_parts),
            main_hand: match settings.main_hand {
                ClientMainHand::Left => MainHand::Left,
                ClientMainHand::Right => MainHand::Right,
            },
            text_filtering_enabled: !settings.disable_text_filtering,
        }
    }
}
impl From<ClientSettings> for PlayClientSettingsSpec {
    fn from(settings: ClientSettings) -> PlayClientSettingsSpec {
        PlayClientSettingsSpec {
            disable_text_filtering: !settings.text_filtering_enabled,
            locale: settings.locale,
            view_distance: settings.view_distance,
            chat_mode: settings.chat_mode.into(),
            chat_colors: settings.chat_colors,
            displayed_skin_parts: set_into_client_displayed_skin_parts(settings.skin_parts),
            main_hand: match settings.main_hand {
                MainHand::Left => ClientMainHand::Left,
                MainHand::Right => ClientMainHand::Right,
            },
        }
    }
}
