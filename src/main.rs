#![allow(unused_imports)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate simplelog;

use std::{
    self,
    collections::HashMap,
    iter::FromIterator,
    net::{
        SocketAddr,
        TcpListener,
        ToSocketAddrs,
    },
    path::Path,
    sync::{
        mpsc::{
            self,
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
        Id,
        PacketDirection,
        PacketErr,
        RawPacket,
        State,
    },
    uuid::UUID4,
    v1_16_3::{
        HandshakeNextState,
        HandshakeSpec,
        LoginSetCompressionSpec,
        LoginStartSpec,
        Packet753 as PacketLatest,
        Packet753Kind as PacketLatestKind,
        PlayBlockChangeSpec,
        RawPacket753 as RawPacketLatest,
        StatusPongSpec,
        StatusResponseSpec,
    },
};
use simplelog::{
    ColorChoice,
    CombinedLogger,
    Config,
    LevelFilter,
    TermLogger,
    TerminalMode,
};

mod connection;
use crate::connection::handle_reader;

mod config;
use crate::config::{
    get_config,
    ConfigLoadError,
    ConfigSaveError,
    SplinterProxyConfiguration,
};

mod mapping;
use crate::mapping::PacketMap;

mod state;
use crate::state::{
    SplinterClient,
    SplinterServer,
    SplinterServerConnection,
    SplinterState,
};

mod zoning;
use crate::zoning::{
    BasicZoner,
    SquareRegion,
    Vector2,
    Zoner,
};

mod chat;
use crate::chat::init;

fn main() {
    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Trace,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )])
    .expect("Logger failed to initialize");
    info!("Starting Splinter proxy");

    // Zoner test code, implements same structure as splinter-prototype
    // Server 0 is inner 200x200 centered at spawn, Server 1 is everything
    // outside of that box.

    // let mut zoner = BasicZoner::new(
    //     vec![(
    //         0, // Server ID for inner box
    //         SquareRegion::new(
    //             Vector2 {
    //                 x: -100,
    //                 z: -100,
    //             },
    //             Vector2 {
    //                 x: 100,
    //                 z: 100,
    //             },
    //         ),
    //     )],
    //     1, // Fallback server ID
    // );

    // println!(
    //     "zone check: {:?}",
    //     zoner.get_zone(&Vector2 {
    //         x: 101,
    //         z: 100
    //     })
    // );

    let mut map: PacketMap = HashMap::new();
    chat::init(&mut map);

    // map.insert(
    //     PacketLatestKind::PlayBlockChange,
    //     Box::new(|state: Arc<SplinterState>, raw_packet: RawPacketLatest| {
    //         info!("blockupdate");

    //         let packet = match raw_packet.deserialize() {
    //             Ok(packet) => packet,
    //             Err(e) => {
    //                 error!("Failed to deserialize packet: {}", e);
    //                 return MapAction::None;
    //             }
    //         };

    //         {
    //             let mut d = state.id.write().unwrap();
    //             *d += 1;
    //         }

    //         if let PacketLatest::PlayBlockChange(mut data) = packet {
    //             data.block_id = (*state.id.read().unwrap()).into();
    //             MapAction::Client(PacketLatest::PlayBlockChange(data))
    //         } else {
    //             MapAction::Client(packet)
    //         }
    //     }),
    // );

    let packet_map: Arc<PacketMap> = Arc::new(map);
    let state = SplinterState {
        config: RwLock::new(get_config("./config.ron")),
        players: RwLock::new(HashMap::new()),
        servers: RwLock::new(HashMap::new()),
    };
    // single server specific, temporary
    let server_id = state.next_server_id();
    state.servers.write().unwrap().insert(
        server_id,
        SplinterServer {
            id: server_id,
            addr: state
                .config
                .read()
                .unwrap()
                .server_address
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap(),
        },
    );
    listen_for_clients(Arc::new(state), packet_map);
}

/// Listens for incoming connections
///
/// This hands control of new connections to a new thread running [`await_handshake`].
fn listen_for_clients(state: Arc<SplinterState>, packet_map: Arc<PacketMap>) {
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
        let packet_map = Arc::clone(&packet_map);
        let cloned_state = Arc::clone(&state);
        thread::spawn(move || await_handshake(cloned_state, craft_conn, sock_addr, packet_map));
    }
}

/// Waits for a handshake from the provided connection
///
/// Branches into [`handle_status`] and [`handle_login`] depending on the handshake's next state.
fn await_handshake(
    state: Arc<SplinterState>,
    mut craft_conn: CraftTcpConnection,
    sock_addr: SocketAddr,
    packet_map: Arc<PacketMap>,
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
                        HandshakeNextState::Login => {
                            handle_login(state, craft_conn, sock_addr, packet_map)
                        }
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
fn handle_status(
    state: Arc<SplinterState>,
    mut craft_conn: CraftTcpConnection,
    sock_addr: SocketAddr,
) {
    craft_conn.set_state(State::Status);
    if let Err(e) = craft_conn.write_packet(PacketLatest::StatusResponse(StatusResponseSpec {
        response: state
            .config
            .read()
            .unwrap()
            .server_status(Some(state.players.read().unwrap().len() as u32)),
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
fn handle_login(
    state: Arc<SplinterState>,
    mut client_conn: CraftTcpConnection,
    client_addr: SocketAddr,
    packet_map: Arc<PacketMap>,
) {
    client_conn.set_state(State::Login);
    let logindata;
    loop {
        match client_conn.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(RawPacketLatest::LoginStart(body))) => match body.deserialize() {
                Ok(data) => {
                    logindata = data;
                    break;
                }
                Err(e) => {
                    return error!(
                        "Error parsing login start packet from {}: {}",
                        client_addr, e
                    )
                }
            },
            Ok(Some(RawPacketLatest::Handshake(body))) => {
                warn!("Got a second handshake? {:?}", body.deserialize().unwrap());
            }
            Ok(Some(other)) => {
                return error!(
                    "Expected a login packet from {}, got {:?}",
                    client_addr, other
                )
            }
            Ok(None) => {
                return info!(
                    "Connection to {} closed before login packet is received",
                    client_addr
                )
            }
            Err(e) => return error!("Error reading packet from {}: {}", client_addr, e),
        };
    }
    let name = logindata.name;
    info!("\"{}\" is attempting to login from {}", name, client_addr);
    debug!("Connecting \"{}\" to server", name);
    let server_addr = state
        .config
        .read()
        .unwrap()
        .server_address
        .as_str()
        .to_socket_addrs()
        .unwrap()
        .next()
        .unwrap(); // yea
    let mut server_conn = match CraftTcpConnection::connect_server_std(server_addr) {
        Ok(conn) => conn,
        Err(e) => {
            return error!(
                "Failed to connect {} to server at {}: {}",
                name, server_addr, e
            )
        }
    };

    if let Err(e) = server_conn.write_packet(PacketLatest::Handshake(HandshakeSpec {
        version: state.config.read().unwrap().protocol_version.into(),
        server_address: format!("{}", server_addr.ip()),
        server_port: server_addr.port(),
        next_state: HandshakeNextState::Login,
    })) {
        return error!("Failed to write handshake to server {}: {}", server_addr, e);
    }

    server_conn.set_state(State::Login);
    if let Err(e) = server_conn.write_packet(PacketLatest::LoginStart(LoginStartSpec {
        name: name.clone(),
    })) {
        return error!(
            "Failed to write login start to server {}: {}",
            server_addr, e
        );
    }
    // look for potential compression packet
    let next_packet = match server_conn.read_raw_packet::<RawPacketLatest>() {
        Ok(Some(RawPacketLatest::LoginSetCompression(body))) => {
            let threshold = match body.deserialize() {
                Ok(LoginSetCompressionSpec {
                    threshold,
                }) => i32::from(threshold),
                Err(e) => {
                    return error!(
                        "Failed to deserialize compression set packet for {}: {}",
                        name, e
                    )
                }
            };
            debug!(
                "Got compression setting from server for {}: {}",
                name, threshold
            );
            server_conn.set_compression_threshold(if threshold > 0 {
                Some(threshold)
            } else {
                None
            });
            server_conn.read_raw_packet::<RawPacketLatest>()
        }
        other => other,
    };
    let client_uuid = UUID4::random();
    let servers_client_uuid;
    // read login success
    match next_packet {
        Ok(Some(RawPacketLatest::LoginSuccess(body))) => {
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
                        return error!("Failed to send set compression packet to {}: {}", name, e)
                    }
                }
            }
            let mut packet = match body.deserialize() {
                Ok(packet) => packet,
                Err(e) => return error!("Failed to deserialize login success: {}", e),
            };
            servers_client_uuid = packet.uuid;
            packet.uuid = client_uuid;
            match client_conn.write_packet(PacketLatest::LoginSuccess(packet)) {
                Ok(()) => {
                    debug!("Relaying login packet to server for {}", name)
                }
                Err(e) => return error!("Failed to relay login packet to server {}: {}", name, e),
            }
            client_conn.set_state(State::Play);
            server_conn.set_state(State::Play);
        }
        Ok(Some(packet)) => {
            return error!(
                "Expected a login success packet for {}, got {:?}",
                name, packet
            )
        }
        Ok(None) => {
            return info!(
                "Server connection closed before receiving login packet {}",
                name
            )
        }
        Err(e) => return error!("Failed to read packet from server for {}: {}", name, e),
    }

    let (server_reader, server_writer) = server_conn.into_split(); // proxy's connection to the server
    let (client_reader, client_writer) = client_conn.into_split(); // proxy's connection to the client
    let splinter_client = SplinterClient {
        id: state.next_client_id(),
        name: name,
        writer: Mutex::new(client_writer),
        uuid: client_uuid,
        servers: RwLock::new(HashMap::new()),
    };
    // TODO: we'll just grab first available server id for now
    let server_id = *state.servers.read().unwrap().iter().next().unwrap().0;
    splinter_client.servers.write().unwrap().insert(
        server_id,
        SplinterServerConnection {
            addr: server_addr,
            id: server_id,
            writer: Mutex::new(server_writer),
            client_uuid: servers_client_uuid,
        },
    );
    let splinter_client = Arc::new(splinter_client);
    {
        let mut players = state.players.write().unwrap();
        players.insert(splinter_client.id, Arc::clone(&splinter_client));
    }
    let is_alive_arc = Arc::new(RwLock::new(true));
    // client reader
    {
        let client = Arc::clone(&splinter_client);
        let state = Arc::clone(&state);
        let packet_map = Arc::clone(&packet_map);
        let is_alive_arc = Arc::clone(&is_alive_arc);
        thread::spawn(move || {
            handle_reader(
                client,
                state,
                is_alive_arc,
                client_reader,
                packet_map,
                PacketDirection::ServerBound,
            )
        });
    }

    // server reader
    {
        let client = Arc::clone(&splinter_client);
        let state = Arc::clone(&state);
        let state2 = Arc::clone(&state);
        let packet_map = Arc::clone(&packet_map);
        let is_alive_arc = Arc::clone(&is_alive_arc);
        thread::spawn(move || {
            handle_reader(
                client,
                state,
                is_alive_arc,
                server_reader,
                packet_map,
                PacketDirection::ClientBound,
            );
            {
                let mut players = state2.players.write().unwrap();
                players.remove_entry(&splinter_client.id);
            }
        });
    }
}
