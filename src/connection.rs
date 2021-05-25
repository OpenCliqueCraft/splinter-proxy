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
};

use craftio_rs::{
    CraftConnection,
    CraftIo,
    CraftSyncReader,
    CraftSyncWriter,
    CraftTcpConnection,
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
    types::RemainingBytes,
    uuid::UUID4,
    v1_16_3::{
        ClientChatMode,
        ClientDisplayedSkinParts,
        ClientMainHand,
        HandshakeNextState,
        HandshakeSpec,
        LoginSetCompressionSpec,
        LoginStartSpec,
        Packet753 as PacketLatest,
        PlayClientSettingsSpec,
        PlayServerPluginMessageSpec,
        PlayTagsSpec,
        RawPacket753 as RawPacketLatest,
        StatusPongSpec,
        StatusResponseSpec,
    },
};

use crate::{
    config::SplinterProxyConfiguration,
    mapping::PacketMap,
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
        uuid: UUID4,
        server_uuid: Option<UUID4>,
        settings: Option<PlayClientSettingsSpec>,
    }
    let mut client_data = PartialClient {
        name: None,
        server: None,
        uuid: UUID4::random(),
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
                    body.uuid = client_data.uuid;
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
                PacketLatest::PlayJoinGame(body) => {
                    // for now we'll just relay this
                    match client_conn.write_packet(PacketLatest::PlayJoinGame(body)) {
                        Ok(()) => {
                            debug!(
                                "Relaying join game packet to {}",
                                client_data.name.as_ref().unwrap()
                            )
                        }
                        Err(e) => {
                            return error!(
                                "Failed to relay join game packet for {}",
                                client_data.name.as_ref().unwrap()
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
                    if if let None = *state.tags.read().unwrap() {
                        true
                    } else {
                        false
                    } {
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
                    next_sender = PacketDirection::ClientBound;
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
        id: state.next_client_id(),
        name: client_data.name.unwrap(),
        writer: Mutex::new(client_writer),
        uuid: client_data.uuid,
        active_server: RwLock::new(client_data.server.unwrap()),
        servers: RwLock::new(HashMap::new()),
        alive: RwLock::new(true),
        settings: client_data.settings.unwrap(),
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
        let client2 = Arc::clone(&splinter_client);
        let state = Arc::clone(&state);
        let state2 = Arc::clone(&state);
        thread::spawn(move || {
            handle_client_reader(client, state, client_reader);
            let mut players = state2.players.write().unwrap();
            players.remove_entry(&client2.id);
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
    while *client.alive.read().unwrap() {
        match reader.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(raw_packet)) => {
                if match state.client_packet_map.get(&raw_packet.kind()) {
                    Some(entry) => entry(&client, &state, &raw_packet),
                    None => true,
                } {
                    if let Err(e) = client
                        .servers
                        .read()
                        .unwrap()
                        .get(&client.active_server.read().unwrap())
                        .unwrap()
                        .writer
                        .lock()
                        .unwrap()
                        .write_raw_packet(raw_packet)
                    {
                        error!(
                            "Failed to relay packet to server for {}: {}",
                            client.name, e
                        );
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
    *client.alive.write().unwrap() = false;
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
    while *client.alive.read().unwrap() {
        match reader.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(raw_packet)) => {
                if match state.server_packet_map.get(&raw_packet.kind()) {
                    Some(entry) => entry(&client, &server, &state, &raw_packet),
                    None => true,
                } {
                    if let Err(e) = client.writer.lock().unwrap().write_raw_packet(raw_packet) {
                        error!(
                            "Failed to relay packet to client for {}: {}",
                            client.name, e
                        );
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
    // TODO: server connection closing should not result in client connection closing
    *client.alive.write().unwrap() = false;
    trace!("server reader thread closed for {}", client.name);
}
