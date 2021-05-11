#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate simplelog;

use std::{
    io::BufReader,
    net::{
        SocketAddr,
        TcpListener,
        TcpStream,
        ToSocketAddrs,
    },
    path::Path,
    sync::{
        Arc,
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
        PacketDirection,
        RawPacket,
        State,
    },
    status::{
        StatusPlayersSpec,
        StatusSpec,
        StatusVersionSpec,
    },
    types::{
        BaseComponent,
        Chat,
        TextComponent,
    },
    v1_16_3::{
        HandshakeNextState,
        HandshakeSpec,
        LoginStartSpec,
        Packet753 as PacketLatest,
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

mod config;
use crate::config::{
    ConfigLoadError,
    ConfigSaveError,
    SplinterProxyConfiguration,
};

const CONFIG_PATH: &'static str = "./config.ron";

fn main() -> Result<(), ()> {
    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Trace,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )])
    .expect("Logger failed to initialize");
    info!("Starting Splinter proxy");
    let config = match SplinterProxyConfiguration::load(Path::new(CONFIG_PATH)) {
        Ok(config) => {
            info!("Config loaded from {}", CONFIG_PATH);
            config
        }
        Err(ConfigLoadError::NoFile) => {
            warn!(
                "No config file found at {}. Creating a new one from defaults",
                CONFIG_PATH
            );
            let config = SplinterProxyConfiguration::default();
            match config.save(Path::new(CONFIG_PATH)) {
                Ok(()) => {}
                Err(ConfigSaveError::Create(e)) => {
                    error!("Failed to create file at {}: {}", CONFIG_PATH, e);
                }
                Err(ConfigSaveError::Write(e)) => {
                    error!("Failed to write to {}: {}", CONFIG_PATH, e);
                }
            }
            config
        }
        Err(ConfigLoadError::Io(e)) => {
            error!(
                "Failed to read config file at {}: {} Using default settings",
                CONFIG_PATH, e
            );
            SplinterProxyConfiguration::default()
        }
        Err(ConfigLoadError::De(e)) => {
            error!(
                "Failure to deserialize config file at {}: {}. Using default settings",
                CONFIG_PATH, e
            );
            SplinterProxyConfiguration::default()
        }
    };
    let config = Arc::new(RwLock::new(config));
    let addr = (*config.read().unwrap()).bind_address.clone();
    accept_loop(addr, config.clone())
}

fn accept_loop(
    addr: impl ToSocketAddrs,
    config: Arc<RwLock<SplinterProxyConfiguration>>,
) -> Result<(), ()> {
    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind listener: {}", e);
            return Err(());
        }
    };
    let mut incoming = listener.incoming();
    loop {
        let stream = match incoming.next() {
            Some(Ok(stream)) => stream,
            Some(Err(e)) => {
                error!("Error when receiving incoming stream: {}", e);
                continue;
            }
            None => {
                debug!("No more incoming connections");
                break;
            }
        };
        let peeraddr = stream.peer_addr().unwrap();
        let mut conn = match CraftConnection::from_std_with_state(
            stream,
            PacketDirection::ServerBound,
            State::Handshaking,
        ) {
            Ok(conn) => conn,
            Err(e) => {
                error!("Failed to wrap TCP stream {}: {}", peeraddr, e);
                continue;
            }
        };
        let config = config.clone();
        thread::spawn(move || match conn.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(RawPacketLatest::Handshake(handshake_body))) => {
                match handshake_body.deserialize() {
                    Ok(handshake) => {
                        debug!(
                            "received handshake from {}: ver {}, server {}:{}, next: {:?}",
                            peeraddr,
                            handshake.version,
                            handshake.server_address,
                            handshake.server_port,
                            handshake.next_state
                        );
                        match handshake.next_state {
                            HandshakeNextState::Status => {
                                handle_status(conn, &peeraddr, config.clone())
                            }
                            HandshakeNextState::Login => {
                                handle_login(conn, &peeraddr, config.clone())
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error parsing handshake packet from {}: {}", peeraddr, e)
                    }
                }
            }
            Ok(Some(other)) => {
                error!("Unexpected packet from {}: {:?}", peeraddr, other)
            }
            Ok(None) => info!("Connection with {} closed before handshake", peeraddr),
            Err(e) => {
                error!("Error reading packet from {}: {}", peeraddr, e)
            }
        });
    }
    Ok(())
}

fn handle_status(
    mut conn: CraftConnection<BufReader<TcpStream>, TcpStream>,
    peeraddr: &SocketAddr,
    config: Arc<RwLock<SplinterProxyConfiguration>>,
) {
    conn.set_state(State::Status);
    match conn.write_packet(PacketLatest::StatusResponse(StatusResponseSpec {
        response: (*config.read().unwrap()).server_status(None), // TODO: player count
    })) {
        Err(e) => return error!("Failed to write packet to {}: {}", peeraddr, e),
        Ok(()) => {}
    }
    loop {
        match conn.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(RawPacketLatest::StatusPing(body))) => match body.deserialize() {
                Ok(ping) => {
                    debug!("Got ping {} from {}", ping.payload, peeraddr);
                    match conn.write_packet(PacketLatest::StatusPong(StatusPongSpec {
                        payload: ping.payload,
                    })) {
                        Ok(()) => debug!("Sent pong back to {}", peeraddr),
                        Err(e) => error!("Failed to send pong back to {}: {}", peeraddr, e),
                    }
                }
                Err(e) => {
                    error!("Error parsing ping packet from {}: {}", peeraddr, e)
                }
            },
            Ok(Some(other)) => {
                error!("Unexpected packet from {}: {:?}", peeraddr, other)
            }
            Ok(None) => {
                info!("Connection with {} closed", peeraddr);
                break;
            }
            Err(e) => error!("Error reading packet from {}: {}", peeraddr, e),
        }
    }
}

fn handle_login(
    mut conn: CraftConnection<BufReader<TcpStream>, TcpStream>,
    peeraddr: &SocketAddr,
    config: Arc<RwLock<SplinterProxyConfiguration>>,
) {
    conn.set_state(State::Login);
    let logindata;
    loop {
        match conn.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(RawPacketLatest::LoginStart(body))) => match body.deserialize() {
                Ok(data) => {
                    logindata = data;
                    break;
                }
                Err(e) => {
                    return error!("Error parsing login start packet from {}: {}", peeraddr, e)
                }
            },
            Ok(Some(RawPacketLatest::Handshake(body))) => {
                warn!("Got a second handshake? {:?}", body.deserialize().unwrap());
            }
            Ok(Some(other)) => {
                return error!("Expected a login packet from {}, got {:?}", peeraddr, other)
            }
            Ok(None) => {
                return info!(
                    "Connection to {} closed before login packet is received",
                    peeraddr
                )
            }
            Err(e) => return error!("Error reading packet from {}: {}", peeraddr, e),
        };
    }
    let name = logindata.name;
    info!("\"{}\" is attempting to login from {}", name, peeraddr);
    debug!("Connecting \"{}\" to server", name);
    let server_addr = (*config.read().unwrap()).server_address.clone();
    let mut client_conn = match CraftTcpConnection::connect_server_std(server_addr.as_str()) {
        Ok(conn) => conn,
        Err(e) => {
            return error!(
                "Failed to connect {} to server at {}: {}",
                name, server_addr, e
            )
        }
    };
    let (server_ip, server_port) = match server_addr.split_once(':') {
        Some((ip, port)) => (
            ip,
            match port.parse::<u16>() {
                Ok(port) => port,
                Err(e) => {
                    return error!(
                        "Failed to parse port in address \"{}\" for {}: {}",
                        server_addr, name, e
                    )
                }
            },
        ),
        None => {
            return error!(
                "Failed to split server address \"{}\" into ip and port for {}",
                server_addr, name
            )
        }
    };
    match client_conn.write_packet(PacketLatest::Handshake(HandshakeSpec {
        version: (*config.read().unwrap()).protocol_version.into(),
        server_address: server_ip.into(),
        server_port: server_port,
        next_state: HandshakeNextState::Login,
    })) {
        Ok(()) => debug!("Sent handshake to {} for {}", server_addr, name),
        Err(e) => {
            return error!(
                "Failed to send handshake to {} for {}: {}",
                server_addr, name, e
            )
        }
    }
    client_conn.set_state(State::Login);
    match client_conn.write_packet(PacketLatest::LoginStart(LoginStartSpec {
        name: name.clone(),
    })) {
        Ok(()) => debug!("Sent login to {} for {}", server_addr, name),
        Err(e) => {
            return error!(
                "Failed to send login to {} for {}: {}",
                server_addr, name, e
            )
        }
    }
    // read login success
    match client_conn.read_raw_packet::<RawPacketLatest>() {
        Ok(Some(RawPacketLatest::LoginSuccess(body))) => {
            match conn.write_raw_packet(RawPacketLatest::LoginSuccess(body)) {
                Ok(()) => {
                    trace!("Relaying login packet to client for {}", name)
                }
                Err(e) => return error!("Failed to relay login packet to client {}: {}", name, e),
            }
            client_conn.set_state(State::Play);
            conn.set_state(State::Play);
        }
        Ok(Some(packet)) => {
            return error!(
                "Expected a login success packet for {}, got {:?}",
                name, packet
            )
        }
        Ok(None) => {
            return info!(
                "Client connection closed before receiving login packet {}",
                name
            )
        }
        Err(e) => return error!("Failed to read packet from client for {}: {}", name, e),
    }
    let (mut server_reader, mut server_writer) = client_conn.into_split(); // proxy's connection to the server
    let (mut client_reader, mut client_writer) = conn.into_split(); // proxy's connection to the client
    let is_alive_arc = Arc::new(RwLock::new(true));
    {
        let is_alive_arc = is_alive_arc.clone();
        let name = name.clone();
        thread::spawn(move || {
            // pass data along from client to server
            while *is_alive_arc.read().unwrap() {
                match client_reader.read_raw_packet::<RawPacketLatest>() {
                    Ok(Some(raw_packet)) => match server_writer.write_raw_packet(raw_packet) {
                        Ok(()) => {} /* trace!("Relaying raw packet client to server for {}", name), */
                        Err(e) => error!(
                            "Failed to relay packet client to server for {}: {}",
                            name, e
                        ),
                    },
                    Ok(None) => {
                        info!("Client connection closed for {}", name);
                        break;
                    }
                    Err(e) => {
                        // not sure if we should be doing something here
                        error!("Failed to read packet client to server for {}: {}", name, e);
                    }
                };
            }
            *is_alive_arc.write().unwrap() = false;
            trace!("client to server thread closed for {}", name);
        });
    }

    // pass data along from server to client
    while *is_alive_arc.read().unwrap() {
        match server_reader.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(raw_packet)) => {
                match client_writer.write_raw_packet(raw_packet) {
                    Ok(()) => {} // trace!("Relaying raw packet server to clientfor {}", name),
                    Err(e) => error!(
                        "Failed to relay packet server to client for {}: {}",
                        name, e
                    ),
                }
            }
            Ok(None) => {
                info!("Server connection closed for {}", name);
                break;
            }
            Err(e) => {
                // not sure if we should be doing something here
                error!("Failed to read packet server to client for {}: {}", name, e);
            }
        };
    }
    *is_alive_arc.write().unwrap() = false;
    trace!("server to client thread closed for {}", name);
}
