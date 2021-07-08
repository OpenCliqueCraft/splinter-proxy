use std::{
    collections::HashSet,
    sync::Arc,
};

use anyhow::Context;
use craftio_rs::{
    CraftAsyncReader,
    CraftAsyncWriter,
    CraftIo,
};
use mcproto_rs::{
    protocol::PacketDirection,
    uuid::UUID4,
    v1_17_0::{
        ClientChatMode,
        ClientDisplayedSkinParts,
        ClientMainHand,
        HandshakeNextState,
        HandshakeSpec,
        LoginSetCompressionSpec,
        LoginStartSpec,
        LoginSuccessSpec,
        Packet755,
        PlayClientSettingsSpec,
        PlayServerPluginMessageSpec,
        PlayTagsSpec,
        RawPacket755,
    },
};

use crate::{
    client::{
        ChatMode,
        ClientSettings,
        MainHand,
        SkinPart,
    },
    protocol::{
        AsyncCraftReader,
        AsyncCraftWriter,
        ClientBuilder,
        Tags,
    },
    proxy::SplinterProxy,
    server::SplinterServerConnection,
};

pub async fn handle_client_login_packet(
    next_sender: &mut PacketDirection,
    builder: &mut ClientBuilder<'_>,
    server_conn_reader: &mut Option<AsyncCraftReader>,
    client_conn_reader: &mut (impl CraftAsyncReader + CraftIo + Send + Sync),
) -> anyhow::Result<Option<bool>> {
    let packet = match next_sender {
        PacketDirection::ServerBound => {
            client_conn_reader
                .read_packet_async::<RawPacket755>()
                .await?
        }
        PacketDirection::ClientBound => {
            server_conn_reader
                .as_mut()
                .unwrap()
                .read_packet_async::<RawPacket755>()
                .await?
        }
    };
    if let Some(packet) = packet {
        match packet {
            Packet755::LoginStart(body) => {
                builder.login_start(&body.name, server_conn_reader).await?;
                *next_sender = PacketDirection::ClientBound;
            }
            Packet755::LoginSetCompression(body) => {
                builder
                    .login_set_compression(*body.threshold, server_conn_reader.as_mut().unwrap());
                *next_sender = PacketDirection::ClientBound;
            }
            Packet755::LoginSuccess(body) => {
                builder.proxy.mapping.lock().await.uuids.insert(
                    builder.uuid.unwrap(),
                    (builder.server_conn.as_ref().unwrap().server.id, body.uuid),
                );
                // body.uuid = builder.uuid.unwrap();
                builder
                    .login_success(client_conn_reader, server_conn_reader.as_mut().unwrap())
                    .await?;
                *next_sender = PacketDirection::ClientBound;
            }
            Packet755::PlayJoinGame(mut body) => {
                body.entity_id = builder.proxy.mapping.lock().await.map_eid_server_to_proxy(
                    builder.server_conn.as_ref().unwrap().server.id,
                    body.entity_id,
                );
                builder
                    .client_writer
                    .write_packet_async(Packet755::PlayJoinGame(body))
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
            Packet755::PlayClientPluginMessage(_body) => {
                //..
                *next_sender = PacketDirection::ServerBound;
            }
            Packet755::PlayClientSettings(body) => {
                builder.play_client_settings(body.clone().into()).await?;
                *next_sender = PacketDirection::ClientBound;
            }
            packet
            @
            (Packet755::PlayServerDifficulty(_)
            | Packet755::PlayServerPlayerAbilities(_)
            | Packet755::PlayDeclareRecipes(_)
            | Packet755::PlayServerHeldItemChange(_)) => {
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
            Packet755::PlayTags(body) => {
                let tags = Tags::from(&body);
                builder.play_tags(tags).await?;
                return Ok(Some(true));
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
pub async fn send_handshake_v755(
    server_conn: &mut SplinterServerConnection,
    proxy: &Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(Packet755::Handshake(HandshakeSpec {
            version: proxy.protocol.to_number().into(),
            server_address: format!("{}", server_conn.server.address.ip()),
            server_port: server_conn.server.address.port(),
            next_state: HandshakeNextState::Login,
        }))
        .await
        .map_err(|e| e.into())
}
pub async fn send_login_start_v755(
    server_conn: &mut SplinterServerConnection,
    name: impl ToString,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(Packet755::LoginStart(LoginStartSpec {
            name: name.to_string(),
        }))
        .await
        .map_err(|e| e.into())
}
pub async fn send_set_compression_v755(
    writer: &mut AsyncCraftWriter,
    threshold: i32,
) -> anyhow::Result<()> {
    writer
        .write_packet_async(Packet755::LoginSetCompression(LoginSetCompressionSpec {
            threshold: threshold.into(),
        }))
        .await
        .map_err(|e| e.into())
}
pub async fn send_login_success_v755(
    writer: &mut AsyncCraftWriter,
    name: String,
    uuid: UUID4,
) -> anyhow::Result<()> {
    writer
        .write_packet_async(Packet755::LoginSuccess(LoginSuccessSpec {
            username: name,
            uuid,
        }))
        .await
        .map_err(|e| e.into())
}
pub async fn send_brand_v755(
    writer: &mut AsyncCraftWriter,
    brand: impl AsRef<str>,
) -> anyhow::Result<()> {
    writer
        .write_packet_async(Packet755::PlayServerPluginMessage(
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
pub async fn send_client_settings_v755(
    server_conn: &mut SplinterServerConnection,
    settings: ClientSettings,
) -> anyhow::Result<()> {
    server_conn
        .writer
        .get_mut()
        .write_packet_async(Packet755::PlayClientSettings(settings.into()))
        .await
        .map_err(|e| e.into())
}
pub async fn send_tags_v755(writer: &mut AsyncCraftWriter, tags: &Tags) -> anyhow::Result<()> {
    writer
        .write_packet_async(Packet755::PlayTags(PlayTagsSpec::from(tags)))
        .await
        .map_err(|e| e.into())
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
