use std::{
    collections::HashSet,
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
    v1_16_3::{
        ClientChatMode,
        ClientDisplayedSkinParts,
        ClientMainHand,
        HandshakeNextState,
        HandshakeSpec,
        LoginSetCompressionSpec,
        LoginStartSpec,
        Packet753,
        PlayClientSettingsSpec,
        PlayServerPluginMessageSpec,
        PlayTagsSpec,
        RawPacket753,
    },
};
use smol::{
    future,
    lock::Mutex,
};

use crate::{
    client::{
        ChatMode,
        ClientSettings,
        MainHand,
        SkinPart,
        SplinterClient,
    },
    protocol::{
        version,
        AsyncCraftConnection,
        ConnectionVersion,
        Tags,
    },
    proxy::SplinterProxy,
    server::{
        self,
        SplinterServerConnection,
    },
};

pub async fn handle_client_login(
    mut conn: AsyncCraftConnection,
    addr: SocketAddr,
    proxy: Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    conn.set_state(State::Login);
    let (mut client_conn_reader, client_conn_writer) = conn.into_split();
    let mut client = SplinterClient::<version::V753>::new(String::new(), client_conn_writer);
    let mut server_conn: Option<AsyncCraftConnection> = None;
    let mut server_id_opt: Option<u64> = None;
    let mut server_opt = None;
    loop {
        let packet = if server_conn.is_none() {
            client_conn_reader
                .read_packet_async::<RawPacket753>()
                .await?
        } else {
            future::or(
                client_conn_reader.read_packet_async::<RawPacket753>(),
                server_conn
                    .as_mut()
                    .unwrap()
                    .read_packet_async::<RawPacket753>(),
            )
            .await?
        };
        if let Some(packet) = packet {
            match packet {
                Packet753::LoginStart(data) => {
                    client.set_name(data.name);
                    info!("\"{}\" logging in from {}", &client.name, addr);
                    client.active_server_id = 0u64; // todo: zoning
                    let server = Arc::clone(
                        proxy
                            .servers
                            .read()
                            .unwrap()
                            .get(&client.active_server_id)
                            .unwrap(),
                    );
                    let mut server_connection = match server::connect(&server).await {
                        Ok(conn) => conn,
                        Err(e) => bail!("Failed to connect client to server: {}", e),
                    };
                    info!(
                        "Connection for client \"{}\" initiated with {}",
                        &client.name, server.address
                    );
                    if let Err(e) = server_connection
                        .write_packet_async(Packet753::Handshake(HandshakeSpec {
                            version: proxy.protocol.to_number().into(),
                            server_address: format!("{}", server.address.ip()),
                            server_port: server.address.port(),
                            next_state: HandshakeNextState::Login,
                        }))
                        .await
                    {
                        bail!(
                            "Failed to write handshake to server {}, {}: {}",
                            client.active_server_id,
                            server.address,
                            e
                        );
                    }
                    server_connection.set_state(State::Login);
                    if let Err(e) = server_connection
                        .write_packet_async(Packet753::LoginStart(LoginStartSpec {
                            name: client.name.clone(),
                        }))
                        .await
                    {
                        bail!(
                            "Failed to write login start packet to server {}, {}: {}",
                            client.active_server_id,
                            server.address,
                            e
                        );
                    }
                    server_id_opt = Some(client.active_server_id);
                    server_opt = Some(server);
                    server_conn = Some(server_connection);
                }
                Packet753::LoginSetCompression(body) => {
                    let threshold = *body.threshold;
                    server_conn
                        .as_mut()
                        .unwrap() // if we're getting a set compression packet, there should be a server
                        .set_compression_threshold(if threshold > 0 {
                            Some(threshold)
                        } else {
                            None
                        });
                }
                Packet753::LoginSuccess(mut body) => {
                    if let Some(threshold) = proxy.config.compression_threshold {
                        if let Err(e) = client
                            .writer
                            .lock()
                            .await
                            .write_packet_async(Packet753::LoginSetCompression(
                                LoginSetCompressionSpec {
                                    threshold: threshold.into(),
                                },
                            ))
                            .await
                        {
                            bail!(
                                "Failed to send compression packet to {}: {}",
                                client.name,
                                e
                            );
                        }
                    }
                    let mut lock = proxy.mapping.lock().await;
                    lock.uuids
                        .insert(client.uuid, (client.active_server_id, body.uuid));
                    body.uuid = lock.map_uuid_server_to_proxy(client.active_server_id, body.uuid);
                    client
                        .writer
                        .lock()
                        .await
                        .write_packet_async(Packet753::LoginSuccess(body))
                        .await
                        .map_err(|e| {
                            anyhow!(
                                "Failed to relay login packet to client {}: {}",
                                client.name,
                                e
                            )
                        })?;
                    client_conn_reader.set_state(State::Play);
                    client.writer.lock().await.set_state(State::Play);
                    server_conn.as_mut().unwrap().set_state(State::Play);
                    client
                        .writer
                        .lock()
                        .await
                        .write_packet_async(Packet753::PlayServerPluginMessage(
                            PlayServerPluginMessageSpec {
                                channel: "minecraft:brand".into(),
                                data: [&[6u8], "Splinter".as_bytes()].concat().into(),
                            },
                        ))
                        .await
                        .map_err(|e| {
                            anyhow!("Failed to send brand to client {}: {}", client.name, e)
                        })?;
                }
                Packet753::PlayJoinGame(mut body) => {
                    body.entity_id = proxy
                        .mapping
                        .lock()
                        .await
                        .map_eid_server_to_proxy(client.active_server_id, body.entity_id);
                    client
                        .writer
                        .lock()
                        .await
                        .write_packet_async(Packet753::PlayJoinGame(body))
                        .await
                        .map_err(|e| {
                            anyhow!(
                                "Failed to relay join game packet for {}: {}",
                                client.name,
                                e
                            )
                        })?;
                }
                Packet753::PlayClientPluginMessage(body) => {
                    //...
                }
                Packet753::PlayClientSettings(body) => {
                    *client.settings.lock().await = body.clone().into();
                    server_conn
                        .as_mut()
                        .unwrap()
                        .write_packet_async(Packet753::PlayClientSettings(body))
                        .await
                        .map_err(|e| {
                            anyhow!(
                                "Failed to relay client settings from {} to server {}: {}",
                                &client.name,
                                client.active_server_id,
                                e
                            )
                        })?;
                    if let Some(tags) = proxy.tags.lock().await.as_ref() {
                        let tag_packet = PlayTagsSpec::from(tags);
                        client
                            .writer
                            .lock()
                            .await
                            .write_packet_async(Packet753::PlayTags(tag_packet))
                            .await
                            .map_err(|e| {
                                anyhow!(
                                    "Failed to send tags packet to client {}: {}",
                                    &client.name,
                                    e
                                )
                            })?;
                    }
                }
                packet
                @
                (Packet753::PlayServerDifficulty(_)
                | Packet753::PlayServerPlayerAbilities(_)
                | Packet753::PlayDeclareRecipes(_)) => {
                    client
                        .writer
                        .lock()
                        .await
                        .write_packet_async(packet)
                        .await
                        .map_err(|e| {
                            anyhow!("Failed to relay server packet to {}: {}", &client.name, e)
                        })?;
                }
                Packet753::PlayTags(body) => {
                    if proxy.tags.lock().await.is_none() {
                        let tags = Tags::from(&body);
                        {
                            *proxy.tags.lock().await = Some(tags.clone());
                        }
                        let tag_packet = PlayTagsSpec::from(&tags);
                        client
                            .writer
                            .lock()
                            .await
                            .write_packet_async(Packet753::PlayTags(tag_packet))
                            .await
                            .map_err(|e| {
                                anyhow!(
                                    "Failed to send tags packet to client {}: {}",
                                    &client.name,
                                    e
                                )
                            })?;
                    }
                    break;
                }
                _ => warn!("Unexpected packet from {}: {:?}", addr, packet),
            }
        } else {
            info!(
                "Client \"{}\", {} connection closed during login",
                client.name, addr
            );
            break;
        }
    }
    let (server_conn_reader, server_conn_writer) = server_conn.unwrap().into_split();
    let server_conn_arc = Arc::new(Mutex::new(SplinterServerConnection {
        writer: server_conn_writer,
        server: server_opt.unwrap(),
        alive: true,
    }));
    client
        .servers
        .lock()
        .await
        .insert(client.active_server_id, Arc::clone(&server_conn_arc));
    let client_arc = Arc::new(client);

    // move on to relay loop
    future::zip(
        super::handle_client_relay(
            Arc::clone(&proxy),
            Arc::clone(&client_arc),
            client_conn_reader,
            addr,
        ),
        super::handle_server_relay(
            proxy,
            Arc::clone(&client_arc),
            server_conn_arc,
            server_conn_reader,
        ),
    )
    .await;
    Ok(())
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
        }
    }
}
