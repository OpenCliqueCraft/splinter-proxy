use std::{
    collections::HashMap,
    net::{
        SocketAddr,
        TcpListener,
        ToSocketAddrs,
    },
    sync::{
        mpsc::{
            Receiver,
            Sender,
        },
        Arc,
        Mutex,
        RwLock,
    },
    thread,
    time::{
        Duration,
        SystemTime,
    },
};

use craftio_rs::{
    CraftConnection,
    CraftIo,
    CraftSyncReader,
    CraftSyncWriter,
    CraftTcpConnection,
    WriteError,
};
use mcproto_rs::{
    protocol::{
        HasPacketId,
        HasPacketKind,
        Id,
        PacketDirection,
        RawPacket,
        State,
    },
    types::{
        BaseComponent,
        Chat,
        RemainingBytes,
        TextComponent,
    },
    uuid::UUID4,
    v1_16_3::{
        ClientChatMode,
        ClientDisplayedSkinParts,
        ClientMainHand,
        HandshakeNextState,
        HandshakeSpec,
        LoginSetCompressionSpec,
        LoginStartSpec,
        PlayClientKeepAliveSpec,
        PlayClientSettingsSpec,
        PlayDisconnectSpec,
        PlayServerKeepAliveSpec,
        PlayServerPluginMessageSpec,
        PlayTagsSpec,
        StatusPongSpec,
        StatusResponseSpec,
    },
};

use crate::{
    config::SplinterProxyConfiguration,
    mapping::{
        eid::map_eid,
        uuid::uuid_from_bytes,
        LazyDeserializedPacket,
        PacketMap,
    },
    proto::{
        PacketLatest,
        PacketLatestKind,
        RawPacketLatest,
    },
    state::{
        SplinterClient,
        SplinterServerConnection,
        SplinterState,
        Tags,
    },
    zoning::Vector2,
};

/// Listens for incoming connections
///
/// This hands control of new connections to a new thread running [`await_handshake`].
pub fn listen_for_clients(state: Arc<SplinterState>) {
    let listener = match TcpListener::bind(&state.config.read().unwrap().bind_address) {
        Err(e) => {
            return error!(
                "Failed to bind TCP listener to {}: {}",
                &state.config.read().unwrap().bind_address,
                e
            )
        }
        Ok(listener) => listener,
    };
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                error!("Error when receiving incoming stream: {}", e);
                continue;
            }
        };

        let sock_addr = stream.peer_addr().unwrap();
        let craft_conn = match CraftConnection::from_std_with_state(
            stream,
            PacketDirection::ServerBound,
            State::Handshaking,
        ) {
            Ok(conn) => conn,
            Err(e) => {
                error!("Failed to wrap TCP stream {}: {}", sock_addr, e);
                continue;
            }
        };
        info!("Got connection from {}", sock_addr);
        let cloned_state = Arc::clone(&state);
        thread::spawn(move || await_handshake(cloned_state, craft_conn, sock_addr));
    }
}

/// Waits for a handshake from the provided connection
///
/// Branches into [`handle_status`] and [`handle_login`] depending on the handshake's next state.
pub fn await_handshake(
    state: Arc<SplinterState>,
    mut craft_conn: CraftTcpConnection,
    sock_addr: SocketAddr,
) {
    match craft_conn.read_raw_packet::<RawPacketLatest>() {
        Ok(Some(RawPacketLatest::Handshake(handshake_body))) => {
            match handshake_body.deserialize() {
                Ok(handshake) => {
                    debug!(
                        "received handshake from {}: ver {}, server {}:{}, next: {:?}",
                        sock_addr,
                        handshake.version,
                        handshake.server_address,
                        handshake.server_port,
                        handshake.next_state
                    );
                    match handshake.next_state {
                        HandshakeNextState::Status => handle_status(state, craft_conn, sock_addr),
                        HandshakeNextState::Login => handle_login(state, craft_conn, sock_addr),
                    }
                }
                Err(e) => {
                    error!("Error parsing handshake packet from {}: {}", sock_addr, e)
                }
            }
        }
        Ok(Some(other)) => {
            error!("Unexpected packet from {}: {:?}", sock_addr, other)
        }
        Ok(None) => info!("Connection with {} closed before handshake", sock_addr),
        Err(e) => {
            error!("Error reading packet from {}: {}", sock_addr, e)
        }
    }
}

/// Responds to connection with status response and waits for pings
pub fn handle_status(
    state: Arc<SplinterState>,
    mut craft_conn: CraftTcpConnection,
    sock_addr: SocketAddr,
) {
    craft_conn.set_state(State::Status);
    if let Err(e) = craft_conn.write_packet(PacketLatest::StatusResponse(StatusResponseSpec {
        response: state.config.read().unwrap().server_status(&*state),
    })) {
        return error!("Failed to write status response to {}: {}", sock_addr, e);
    }

    loop {
        match craft_conn.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(RawPacketLatest::StatusPing(body))) => match body.deserialize() {
                Ok(ping) => {
                    debug!("Got ping {} from {}", ping.payload, sock_addr);
                    if let Err(e) =
                        craft_conn.write_packet(PacketLatest::StatusPong(StatusPongSpec {
                            payload: ping.payload,
                        }))
                    {
                        return error!("Failed to write pong to client {}: {}", sock_addr, e);
                    }
                }
                Err(e) => {
                    error!("Error parsing ping packet from {}: {}", sock_addr, e)
                }
            },
            Ok(Some(other)) => {
                error!("Unexpected packet from {}: {:?}", sock_addr, other)
            }
            Ok(None) => {
                info!("Connection with {} closed", sock_addr);
                break;
            }
            Err(e) => error!("Error reading packet from {}: {}", sock_addr, e),
        }
    }
}

/// Writes the given packet to a server
///
/// Preferably use this over directly accessing a server's writer, so that the packet will go
/// through the server packet map
pub fn write_packet_server(
    client: &Arc<SplinterClient>,
    server: &Arc<SplinterServerConnection>,
    state: &Arc<SplinterState>,
    mut lazy_packet: LazyDeserializedPacket,
) -> Result<(), WriteError> {
    if let Some(entry) = state.server_packet_map.0.get(&lazy_packet.kind()) {
        for action in entry.iter() {
            action(client, server, state, &mut lazy_packet);
        }
    }
    if let Ok(packet) = lazy_packet.into_packet() {
        server.writer.lock().unwrap().write_packet(packet)
    } else {
        Ok(())
    }
}

/// Writes the given packet to a client
///
/// Preferably use this over directly accessing a client's writer, so that the packet will go
/// through the client packet map
pub fn write_packet_client(
    client: &Arc<SplinterClient>,
    state: &Arc<SplinterState>,
    mut lazy_packet: LazyDeserializedPacket,
) -> Result<(), WriteError> {
    if let Some(entry) = state.client_packet_map.0.get(&lazy_packet.kind()) {
        for action in entry.iter() {
            action(client, state, &mut lazy_packet);
        }
    }
    if let Ok(packet) = lazy_packet.into_packet() {
        client.writer.lock().unwrap().write_packet(packet)
    } else {
        Ok(())
    }
}

/// A reason for a client to get kicked
pub enum ClientKickReason {
    /// Client failed to send a keep alive packet back in time
    TimedOut,
    /// Client was directly kicked
    Kicked(String, Option<String>),
    /// Server shut down
    Shutdown,
}

impl ClientKickReason {
    pub fn text(&self) -> String {
        match self {
            ClientKickReason::TimedOut => "Timed out".into(),
            ClientKickReason::Kicked(by, reason) => format!(
                "Kicked by {}{}",
                by,
                if let Some(reason) = reason {
                    format!(" because \"{}\"", reason)
                } else {
                    "".into()
                }
            ),
            ClientKickReason::Shutdown => "Server shut down".into(),
        }
    }
}

/// Kicks a client from the proxy
pub fn kick_client(
    client: &Arc<SplinterClient>,
    state: &Arc<SplinterState>,
    reason: ClientKickReason,
) {
    info!("Kicking {}: {}", client.name, reason.text());
    if let Err(e) = write_packet_client(
        client,
        state,
        LazyDeserializedPacket::from_packet(PacketLatest::PlayDisconnect(PlayDisconnectSpec {
            reason: Chat::Text(TextComponent {
                text: reason.text(),
                base: BaseComponent::default(),
            }),
        })),
    ) {
        error!(
            "Failed to send disconnect to client {}, {}: {}",
            client.id, client.name, e
        );
    }
    *client.alive.write().unwrap() = false;
}

/// Handles login sequence between server and client
///
/// After login, packets can be inspected and relayed.
pub fn handle_login(
    state: Arc<SplinterState>,
    mut client_conn: CraftTcpConnection,
    client_addr: SocketAddr,
) {
    struct PartialClient {
        name: Option<String>,
        server: Option<u64>,
        server_addr: Option<SocketAddr>,
        uuid: Option<UUID4>,
        server_uuid: Option<UUID4>,
        settings: Option<PlayClientSettingsSpec>,
    }
    let mut client_data = PartialClient {
        name: None,
        server: None,
        uuid: None,
        server_uuid: None,
        server_addr: None,
        settings: None,
    };
    client_conn.set_state(State::Login);
    let mut next_sender = PacketDirection::ServerBound;
    let mut server_conn: Option<CraftTcpConnection> = None;
    loop {
        match match next_sender {
            PacketDirection::ServerBound => &mut client_conn,
            PacketDirection::ClientBound => server_conn.as_mut().unwrap(),
        }
        .read_packet::<RawPacketLatest>()
        {
            Ok(Some(packet)) => match packet {
                PacketLatest::LoginStart(data) => {
                    let name = data.name;
                    client_data.name = Some(name.clone());
                    client_data.uuid = Some(uuid_from_bytes(
                        format!("OfflinePlayer:{}", name).as_bytes(),
                    ));
                    info!("\"{}\" attempting to log in from {}", name, client_addr);
                    // TODO: grab player location information and find server from that
                    let player_loc = Vector2 {
                        x: 0,
                        z: 0,
                    };
                    let server_id = state.zoner.read().unwrap().get_zone(&player_loc);
                    client_data.server = Some(server_id);
                    let server_addr = state.servers.read().unwrap().get(&server_id).unwrap().addr;
                    client_data.server_addr = Some(server_addr);
                    server_conn = Some(match CraftTcpConnection::connect_server_std(server_addr) {
                        Ok(conn) => conn,
                        Err(e) => {
                            return error!(
                                "Failed to connect {} to server at {}: {}",
                                name, server_addr, e
                            )
                        }
                    });
                    let server_conn = server_conn.as_mut().unwrap();
                    if let Err(e) =
                        server_conn.write_packet(PacketLatest::Handshake(HandshakeSpec {
                            version: state.config.read().unwrap().protocol_version.into(),
                            server_address: format!("{}", server_addr.ip()),
                            server_port: server_addr.port(),
                            next_state: HandshakeNextState::Login,
                        }))
                    {
                        return error!(
                            "Failed to write handshake to server {}: {}",
                            server_addr, e
                        );
                    }

                    server_conn.set_state(State::Login);
                    if let Err(e) =
                        server_conn.write_packet(PacketLatest::LoginStart(LoginStartSpec {
                            name: name.clone(),
                        }))
                    {
                        return error!(
                            "Failed to write login start to server {}: {}",
                            server_addr, e
                        );
                    }
                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::LoginSetCompression(body) => {
                    let threshold = i32::from(body.threshold);
                    debug!(
                        "Got compression setting from server for {}: {}",
                        client_data
                            .name
                            .as_ref()
                            .unwrap_or(&format!("{}", client_addr)),
                        threshold
                    );
                    match server_conn.as_mut() {
                        Some(server_conn) => {
                            server_conn.set_compression_threshold(if threshold > 0 {
                                Some(threshold)
                            } else {
                                None
                            });
                        }
                        None => error!(
                            "Got set compression packet before server connection established?"
                        ),
                    }
                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::LoginSuccess(mut body) => {
                    let name = match client_data.name.as_ref() {
                        Some(name) => name.clone(),
                        None => format!("{}", client_addr),
                    };
                    if let Some(threshold) = state.config.read().unwrap().compression_threshold {
                        match client_conn.write_packet(PacketLatest::LoginSetCompression(
                            LoginSetCompressionSpec {
                                threshold: threshold.into(),
                            },
                        )) {
                            Ok(()) => {
                                debug!("Sent set compression to {} of {}", name, threshold);
                                client_conn.set_compression_threshold(
                                    state.config.read().unwrap().compression_threshold,
                                );
                            }
                            Err(e) => {
                                return error!(
                                    "Failed to send set compression packet to {}: {}",
                                    name, e
                                )
                            }
                        }
                    }
                    client_data.server_uuid = Some(body.uuid);
                    state.uuid_table.write().unwrap().insert(
                        client_data.uuid.unwrap(),
                        (
                            client_data.server.unwrap(),
                            client_data.server_uuid.unwrap(),
                        ),
                    );
                    body.uuid = client_data.uuid.unwrap();
                    match client_conn.write_packet(PacketLatest::LoginSuccess(body)) {
                        Ok(()) => {
                            debug!("Relaying login packet to server for {}", name)
                        }
                        Err(e) => {
                            return error!("Failed to relay login packet to server {}: {}", name, e)
                        }
                    }
                    client_conn.set_state(State::Play);
                    server_conn.as_mut().unwrap().set_state(State::Play);
                    match client_conn.write_packet(PacketLatest::PlayServerPluginMessage(
                        PlayServerPluginMessageSpec {
                            channel: "minecraft:brand".into(),
                            data: "Splinter".as_bytes().to_vec().into(), // TODO: put in config or something
                        },
                    )) {
                        Ok(()) => debug!("Sent brand to client {}", name),
                        Err(e) => return error!("Failed to send brand to client {}: {}", name, e),
                    }
                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::PlayJoinGame(mut body) => {
                    // for now we'll just relay this
                    trace!("Got join game");
                    body.entity_id =
                        map_eid(&state, client_data.server.unwrap(), body.entity_id.into()).into();
                    trace!("Mapped eid");
                    match client_conn.write_packet(PacketLatest::PlayJoinGame(body)) {
                        Ok(()) => {
                            debug!(
                                "Relaying join game packet to {}",
                                client_data.name.as_ref().unwrap()
                            )
                        }
                        Err(e) => {
                            return error!(
                                "Failed to relay join game packet for {}: {}",
                                client_data.name.as_ref().unwrap(),
                                e
                            )
                        }
                    }
                    next_sender = PacketDirection::ServerBound;
                }
                PacketLatest::PlayClientPluginMessage(body) => {
                    debug!(
                        "Serverbound Channel \"{}\" for {}: {:?}",
                        body.channel,
                        client_data.name.as_ref().unwrap(),
                        body.data
                    );
                    match body.channel.as_str() {
                        "minecraft:brand" => {}
                        _ => {}
                    }
                    next_sender = PacketDirection::ServerBound;
                }
                PacketLatest::PlayClientSettings(body) => {
                    client_data.settings = Some(body.clone());
                    match server_conn
                        .as_mut()
                        .unwrap()
                        .write_packet(PacketLatest::PlayClientSettings(body))
                    {
                        Ok(()) => debug!(
                            "Relayed client settings from {} to server {}",
                            client_data.name.as_ref().unwrap(),
                            client_data.server_addr.as_ref().unwrap(),
                        ),
                        Err(e) => {
                            return error!(
                                "Failed to relay client settings from {} to server {}: {}",
                                client_data.name.as_ref().unwrap(),
                                client_data.server_addr.as_ref().unwrap(),
                                e
                            )
                        }
                    }
                    // we will also send our tag packet at this point
                    if let Some(tags) = state.tags.read().unwrap().as_ref() {
                        let tag_packet = PlayTagsSpec::from(tags);
                        match client_conn.write_packet(PacketLatest::PlayTags(tag_packet)) {
                            Ok(()) => debug!(
                                "Sent tags packet to client {}",
                                client_data.name.as_ref().unwrap()
                            ),
                            Err(e) => {
                                return error!(
                                    "Failed to send tags packet to client {}: {}",
                                    client_data.name.as_ref().unwrap(),
                                    e
                                )
                            }
                        }
                    }

                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::PlayServerPluginMessage(body) => {
                    debug!(
                        "Clientbound Channel \"{}\" for {}: {:?}",
                        body.channel,
                        client_data.name.as_ref().unwrap(),
                        body.data
                    );
                    match body.channel.as_str() {
                        "minecraft:brand" => {}
                        _ => {}
                    }
                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::PlayServerDifficulty(body) => {
                    match client_conn.write_packet(PacketLatest::PlayServerDifficulty(body)) {
                        Ok(()) => debug!(
                            "Relayed server difficulty packet to {}",
                            client_data.name.as_ref().unwrap()
                        ),
                        Err(e) => {
                            return error!(
                                "Failed to relay server difficulty packet to {}: {}",
                                client_data.name.as_ref().unwrap(),
                                e
                            )
                        }
                    }
                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::PlayServerPlayerAbilities(body) => {
                    match client_conn.write_packet(PacketLatest::PlayServerPlayerAbilities(body)) {
                        Ok(()) => debug!(
                            "Relayed player abilities to {}",
                            client_data.name.as_ref().unwrap()
                        ),
                        Err(e) => {
                            return error!(
                                "Failed to relay player abilities to {}: {}",
                                client_data.name.as_ref().unwrap(),
                                e
                            )
                        }
                    }
                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::PlayServerHeldItemChange(body) => {
                    match client_conn.write_packet(PacketLatest::PlayServerHeldItemChange(body)) {
                        Ok(()) => debug!(
                            "Relayed player held item to {}",
                            client_data.name.as_ref().unwrap()
                        ),
                        Err(e) => {
                            return error!(
                                "Failed to relay player held item to {}: {}",
                                client_data.name.as_ref().unwrap(),
                                e
                            )
                        }
                    }
                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::PlayDeclareRecipes(body) => {
                    match client_conn.write_packet(PacketLatest::PlayDeclareRecipes(body)) {
                        Ok(()) => debug!(
                            "Relayed declared recipes to {}",
                            client_data.name.as_ref().unwrap()
                        ),
                        Err(e) => {
                            return error!(
                                "Failed to relay declared recipes to {}: {}",
                                client_data.name.as_ref().unwrap(),
                                e
                            )
                        }
                    }
                    next_sender = PacketDirection::ClientBound;
                }
                PacketLatest::PlayTags(body) => {
                    trace!("Server sent play tags packet");
                    if state.tags.read().unwrap().is_none() {
                        // if no known tags, store and relay here
                        let tags = Tags::from(&body);
                        let cloned_tags = tags.clone();
                        let mut tag_lock = state.tags.write().unwrap();
                        *tag_lock = Some(cloned_tags);
                        drop(tag_lock);
                        debug!("Saved tags");
                        let tag_packet = PlayTagsSpec::from(&tags);
                        match client_conn.write_packet(PacketLatest::PlayTags(tag_packet)) {
                            Ok(()) => debug!(
                                "Sent tags packet to client {}",
                                client_data.name.as_ref().unwrap()
                            ),
                            Err(e) => {
                                return error!(
                                    "Failed to send tags packet to client {}: {}",
                                    client_data.name.as_ref().unwrap(),
                                    e
                                )
                            }
                        }
                    }
                    // next_sender = PacketDirection::ClientBound;
                    break;
                }
                _ => error!(
                    "Unexpected packet from {} during login: {:?}",
                    client_addr, packet
                ),
            },
            Ok(None) => info!("Connection to {} closed during login", client_addr),
            Err(e) => error!("Error reading packet from {}: {}", client_addr, e),
        }
    }
    let (server_reader, server_writer) = server_conn.unwrap().into_split(); // proxy's connection to the server
    let (client_reader, client_writer) = client_conn.into_split(); // proxy's connection to the client
    let splinter_client = SplinterClient {
        id: state.player_id_gen.lock().unwrap().take_id(),
        name: client_data.name.unwrap(),
        writer: Mutex::new(client_writer),
        uuid: client_data.uuid.unwrap(),
        active_server: RwLock::new(client_data.server.unwrap()),
        servers: RwLock::new(HashMap::new()),
        alive: RwLock::new(true),
        settings: client_data.settings.unwrap(),
        keep_alive_waiting: RwLock::new(vec![]),
    };
    let server_client_conn = Arc::new(SplinterServerConnection {
        addr: client_data.server_addr.unwrap(),
        id: client_data.server.unwrap(),
        writer: Mutex::new(server_writer),
        client_uuid: client_data.server_uuid.unwrap(),
    });
    splinter_client
        .servers
        .write()
        .unwrap()
        .insert(server_client_conn.id, Arc::clone(&server_client_conn));
    let splinter_client = Arc::new(splinter_client);
    {
        let mut players = state.players.write().unwrap();
        players.insert(splinter_client.id, Arc::clone(&splinter_client));
    }
    // client reader
    {
        let client = Arc::clone(&splinter_client);
        let state = Arc::clone(&state);
        thread::spawn(move || {
            handle_client_reader(client, state, client_reader);
        });
    }

    // server reader
    {
        let client = Arc::clone(&splinter_client);
        let state = Arc::clone(&state);
        thread::spawn(move || {
            handle_server_reader(client, server_client_conn, state, server_reader);
        });
    }
}

/// Handles reading a connection and deciding what to do with data
///
/// `client` contains the state of the client.
///
/// `state` is the state of the proxy.
///
/// `is_alive` is a [`Arc`]<[`RwLock`]<[`bool`]>>. The as long as `is_alive` is true, then the reader
/// will continue reading. The reader can also turn off `is_alive` itself.
///
/// `packet_map` is an [`Arc`]<[`PacketMap`]> so that it can correctly determine what to do with certain packets.
///
/// `direction` is the packet flow, whether packets are coming from server (client bound) or coming
/// from client (server bound)
pub fn handle_client_reader(
    client: Arc<SplinterClient>,
    state: Arc<SplinterState>,
    mut reader: impl CraftSyncReader,
) {
    'outer: while *client.alive.read().unwrap() {
        match reader.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(raw_packet)) => {
                let mut lazy_packet = LazyDeserializedPacket::new(&raw_packet);
                if match state.client_packet_map.0.get(&raw_packet.kind()) {
                    Some(entry) => {
                        entry.is_empty()
                            || entry.iter().fold(false, |acc, action| {
                                acc || action(&client, &state, &mut lazy_packet)
                            })
                    }
                    None => true,
                } {
                    let server = client.server();
                    let mut relay_packet = true;
                    if let Some(entry) = state.server_packet_map.0.get(&raw_packet.kind()) {
                        // only serverbound actions will be found; should not conflict with the clientbound ones
                        relay_packet = entry.is_empty()
                            || entry.iter().fold(false, |acc, action| {
                                acc || action(&client, &server, &state, &mut lazy_packet)
                            });
                    }
                    if relay_packet {
                        let mut writer = server.writer.lock().unwrap();
                        if let Err(e) = if lazy_packet.is_deserialized() {
                            writer.write_packet(match lazy_packet.into_packet() {
                                Ok(packet) => packet,
                                Err(e) => {
                                    error!("Failed to parse packet from {}: {}", client.name, e);
                                    continue 'outer;
                                }
                            })
                        } else {
                            writer.write_raw_packet(raw_packet)
                        } {
                            error!(
                                "Failed to relay packet to server for {}: {}",
                                client.name, e
                            );
                        }
                    }
                }
            }
            Ok(None) => {
                break;
            }
            Err(e) => {
                error!("Failed to read packet for {}: {}", client.name, e);
            }
        }
    }
    if let Some(client) = state.players.write().unwrap().remove(&client.id) {
        *client.alive.write().unwrap() = false;
    }
    trace!("client reader thread closed for {}", client.name);
}

/// Handles reading a connection and deciding what to do with data
///
/// `client` contains the state of the client.
///
/// `state` is the state of the proxy.
///
/// `is_alive` is a [`Arc`]<[`RwLock`]<[`bool`]>>. The as long as `is_alive` is true, then the reader
/// will continue reading. The reader can also turn off `is_alive` itself.
///
/// `packet_map` is an [`Arc`]<[`PacketMap`]> so that it can correctly determine what to do with certain packets.
///
/// `direction` is the packet flow, whether packets are coming from server (client bound) or coming
/// from client (server bound)
pub fn handle_server_reader(
    client: Arc<SplinterClient>,
    server: Arc<SplinterServerConnection>,
    state: Arc<SplinterState>,
    mut reader: impl CraftSyncReader,
) {
    'outer: while *client.alive.read().unwrap() {
        match reader.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(raw_packet)) => {
                let mut lazy_packet = LazyDeserializedPacket::new(&raw_packet);
                if match state.server_packet_map.0.get(&raw_packet.kind()) {
                    Some(entry) => {
                        entry.is_empty()
                            || entry.iter().fold(false, |acc, action| {
                                acc || action(&client, &server, &state, &mut lazy_packet)
                            })
                    }
                    None => true,
                } {
                    let mut relay_packet = true;
                    if let Some(entry) = state.client_packet_map.0.get(&raw_packet.kind()) {
                        // only serverbound actions will be found; should not conflict with the clientbound ones
                        relay_packet = entry.is_empty()
                            || entry.iter().fold(false, |acc, action| {
                                acc || action(&client, &state, &mut lazy_packet)
                            });
                    }
                    if relay_packet {
                        let mut writer = client.writer.lock().unwrap();
                        if let Err(e) = if lazy_packet.is_deserialized() {
                            writer.write_packet(match lazy_packet.into_packet() {
                                Ok(packet) => packet,
                                Err(e) => {
                                    error!(
                                        "Failed to parse packet for client {} from server {}: {}",
                                        client.name, server.addr, e
                                    );
                                    continue 'outer;
                                }
                            })
                        } else {
                            writer.write_raw_packet(raw_packet)
                        } {
                            error!(
                                "Failed to relay packet to client for {}: {}",
                                client.name, e
                            );
                        }
                    }
                }
            }
            Ok(None) => {
                break;
            }
            Err(e) => {
                error!("Failed to read packet for {}: {}", client.name, e);
            }
        }
    }
    trace!("server reader thread closed for {}", client.name);
}

/// Gets the current unix time in milliseconds
pub fn unix_time_millis() -> u128 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur.as_millis(),
        Err(e) => {
            warn!("System time before unix epoch?: {}", e);
            0
        }
    }
}

pub fn init(state: &mut SplinterState) {
    state.client_packet_map.add_action(
        PacketLatestKind::PlayClientKeepAlive,
        Box::new(|client, _state, lazy_packet| {
            if let Ok(PacketLatest::PlayClientKeepAlive(body)) = lazy_packet.packet() {
                let waiting = &mut *client.keep_alive_waiting.write().unwrap();
                if let Some(ind) = waiting.iter().position(|millis| *millis == body.id) {
                    waiting.remove(ind);
                }
            }
            false
        }),
    );
    state.server_packet_map.add_action(
        PacketLatestKind::PlayServerKeepAlive,
        Box::new(|client, server, state, lazy_packet| {
            if let Ok(PacketLatest::PlayServerKeepAlive(body)) = lazy_packet.packet() {
                if let Err(e) = write_packet_server(
                    client,
                    server,
                    state,
                    LazyDeserializedPacket::from_packet(PacketLatest::PlayClientKeepAlive(
                        PlayClientKeepAliveSpec {
                            id: body.id,
                        },
                    )),
                ) {
                    error!(
                        "Failed to write keep alive packet to server {}: {}",
                        server.id, e
                    );
                }
            }
            false
        }),
    );
}

pub fn init_post(state: Arc<SplinterState>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(15));
            let clients = state.players.read().unwrap();
            let keep_alive_millis = unix_time_millis() as i64;
            let packet = PacketLatest::PlayServerKeepAlive(PlayServerKeepAliveSpec {
                id: keep_alive_millis,
            });
            for (id, client) in clients.iter() {
                if let Some(longest_millis) = client.keep_alive_waiting.read().unwrap().get(0) {
                    if keep_alive_millis - longest_millis > 30 * 1000 {
                        // if it's been more than 30 seconds since the longest awaiting keep alive packet
                        // disconnect the client
                        kick_client(client, &state, ClientKickReason::TimedOut);
                        error!(
                            "Client {} disconnected because they failed to return keep alive packets",
                            client.name
                        );
                        continue;
                    }
                }
                if let Err(e) = write_packet_client(
                    client,
                    &state,
                    LazyDeserializedPacket::from_packet(packet.clone()),
                ) {
                    error!(
                        "Failed to send keep alive packet to client {}, {}: {}",
                        id, client.name, e
                    );
                } else {
                    client
                        .keep_alive_waiting
                        .write()
                        .unwrap()
                        .push(keep_alive_millis);
                }
            }
        }
    });
}
