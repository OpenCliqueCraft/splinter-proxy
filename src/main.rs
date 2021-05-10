#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate simplelog;

use std::{
    net::{
        TcpListener,
        TcpStream,
        ToSocketAddrs,
        SocketAddr,
    },
    io::{
        BufReader,
        self,
    },
    fs::File,
    thread,
    sync::{
        RwLock,
        Arc,
    },
};

use craftio_rs::{
    CraftConnection,
    CraftTcpConnection,
    CraftReader,
    CraftWriter,
    CraftSyncReader,
    CraftSyncWriter,
    CraftIo,
    WriteError,
    ReadError,
};
use mcproto_rs::{
    protocol::{
        PacketDirection,
        State,
    },
    v1_16_3::{
        RawPacket753 as RawPacketLatest,
        Packet753 as PacketLatest,
        HandshakeSpec,
        HandshakeNextState,
        StatusResponseSpec,
        StatusPongSpec,
        LoginSuccessSpec,
        LoginStartSpec,
    },
    status::{
        StatusSpec,
        StatusVersionSpec,
        StatusPlayersSpec,
    },
    types::{
        VarInt,
        Chat,
        BaseComponent,
        TextComponent,
        ColorCode,
    },
};
use simplelog::{
    CombinedLogger,
    TermLogger,
    WriteLogger,
    LevelFilter,
    Config,
    TerminalMode,
    ColorChoice,
};

const PROTOCOL_VERSION: i32 = 753;
const GAME_VERSION: &'static str = "1.16.3";
const DEFAULT_ADDRESS: &'static str = "127.0.0.1:25565";
const DEFAULT_SERVER_ADDRESS: &'static str = "127.0.0.1:25400";

lazy_static! {
    static ref SERVER_STATUS: StatusSpec = StatusSpec {
        version: Some(StatusVersionSpec {
            name: GAME_VERSION.into(),
            protocol: PROTOCOL_VERSION,
        }),
        players: StatusPlayersSpec {
            max: 1,
            online: 0,
            sample: vec!(),
        },
        description: Chat::Text(TextComponent {
            text: "Splinter Proxy".into(),
            base: BaseComponent::default(),
        }),
        favicon: None,
    };
}

fn main() -> Result<(), ()> {
    CombinedLogger::init(vec![
        TermLogger::new(LevelFilter::Trace, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
    ]).expect("Logger failed to initialize");
    info!("Starting Splinter proxy");
    let addr = DEFAULT_ADDRESS;
    accept_loop(addr)
}

fn accept_loop(addr: impl ToSocketAddrs) -> Result<(), ()> {
    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind listener: {}", e);
            return Err(());
        },
    };
    let mut incoming = listener.incoming();
    loop {
        let stream = match incoming.next() {
            Some(Ok(stream)) => stream,
            Some(Err(e)) => {
                error!("Error when receiving incoming stream: {}", e);
                continue;
            },
            None => {
                debug!("No more incoming connections");
                break;
            },
        };
        let peeraddr = stream.peer_addr().unwrap();
        let mut conn = match CraftConnection::from_std_with_state(stream, PacketDirection::ServerBound, State::Handshaking) {
            Ok(conn) => conn,
            Err(e) => {
                error!("Failed to wrap TCP stream {}: {}", peeraddr, e);
                continue;
            },
        };
        thread::spawn(move || {
            match conn.read_raw_packet::<RawPacketLatest>() {
                Ok(Some(RawPacketLatest::Handshake(handshake_body))) => {
                    match handshake_body.deserialize() {
                        Ok(handshake) => {
                            debug!("received handshake from {}: ver {}, server {}:{}, next: {:?}", peeraddr,
                                handshake.version, handshake.server_address, handshake.server_port, handshake.next_state
                            );
                            match handshake.next_state {
                                HandshakeNextState::Status => handle_status(conn, &peeraddr),
                                HandshakeNextState::Login => handle_login(conn, &peeraddr),
                            }
                        },
                        Err(e) => error!("Error parsing handshake packet from {}: {}", peeraddr, e),
                    }
                },
                Ok(Some(other)) => error!("Unexpected packet from {}: {:?}", peeraddr, other),
                Ok(None) => info!("Connection with {} closed before handshake", peeraddr),
                Err(e) => error!("Error reading packet from {}: {}", peeraddr, e),
            }
        });
    }
    Ok(())
}

fn handle_status(mut conn: CraftConnection<BufReader<TcpStream>, TcpStream>, peeraddr: &SocketAddr) {
    conn.set_state(State::Status);
    match conn.write_packet(PacketLatest::StatusResponse(StatusResponseSpec { response: SERVER_STATUS.clone() })) {
        Err(e) => return error!("Failed to write packet to {}: {}", peeraddr, e),
        Ok(()) => {},
    }
    loop {
        match conn.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(RawPacketLatest::StatusPing(body))) => {
                match body.deserialize() {
                    Ok(ping) => {
                        debug!("Got ping {} from {}", ping.payload, peeraddr);
                        match conn.write_packet(PacketLatest::StatusPong(StatusPongSpec { payload: ping.payload })) {
                            Ok(()) => debug!("Sent pong back to {}", peeraddr),
                            Err(e) => error!("Failed to send pong back to {}: {}", peeraddr, e),
                        }
                    },
                    Err(e) => error!("Error parsing ping packet from {}: {}", peeraddr, e),
                }
            },
            Ok(Some(other)) => error!("Unexpected packet from {}: {:?}", peeraddr, other),
            Ok(None) => {
                info!("Connection with {} closed", peeraddr);
                break;
            },
            Err(e) => error!("Error reading packet from {}: {}", peeraddr, e),
        }
    }
}

fn handle_login(mut conn: CraftConnection<BufReader<TcpStream>, TcpStream>, peeraddr: &SocketAddr) {
    conn.set_state(State::Login);
    let logindata;
    loop {
        match conn.read_raw_packet::<RawPacketLatest>() {
            Ok(Some(RawPacketLatest::LoginStart(body))) => match body.deserialize() {
                Ok(data) => {
                    logindata = data;
                    break;
                },
                Err(e) => return error!("Error parsing login start packet from {}: {}", peeraddr, e),
            },
            Ok(Some(RawPacketLatest::Handshake(body))) => {
                warn!("Got a second handshake? {:?}", body.deserialize().unwrap());
            }
            Ok(Some(other)) => return error!("Expected a login packet from {}, got {:?}", peeraddr, other),
            Ok(None) => return info!("Connection to {} closed before login packet is received", peeraddr),
            Err(e) => return error!("Error reading packet from {}: {}", peeraddr, e),
        };
    }
    let name = logindata.name;
    info!("\"{}\" is attempting to login from {}", name, peeraddr);
    debug!("Connecting \"{}\" to server", name);
    let server_addr = DEFAULT_SERVER_ADDRESS;
    let mut client_conn = match CraftTcpConnection::connect_server_std(server_addr) {
        Ok(conn) => conn,
        Err(e) => return error!("Failed to connect {} to server at {}: {}", name, server_addr, e),
    };
    let (server_ip, server_port) = match server_addr.split_once(':') {
        Some((ip, port)) => (ip, match port.parse::<u16>() {
            Ok(port) => port,
            Err(e) => return error!("Failed to parse port in address \"{}\" for {}: {}", server_addr, name, e),
        }),
        None => return error!("Failed to split server address \"{}\" into ip and port for {}", server_addr, name),
    };
    match client_conn.write_packet(PacketLatest::Handshake(HandshakeSpec {
        version: PROTOCOL_VERSION.into(),
        server_address: server_ip.into(),
        server_port: server_port,
        next_state: HandshakeNextState::Login,
    })) {
        Ok(()) => debug!("Sent handshake to {} for {}", server_addr, name),
        Err(e) => return error!("Failed to send handshake to {} for {}: {}", server_addr, name, e),
    }
    client_conn.set_state(State::Login);
    match client_conn.write_packet(PacketLatest::LoginStart(LoginStartSpec { name: name.clone() })) {
        Ok(()) => debug!("Sent login to {} for {}", server_addr, name),
        Err(e) => return error!("Failed to send login to {} for {}: {}", server_addr, name, e),
    }
    // read login success
    match client_conn.read_raw_packet::<RawPacketLatest>() {
        Ok(Some(RawPacketLatest::LoginSuccess(body))) => {
            match conn.write_raw_packet(RawPacketLatest::LoginSuccess(body)) {
                Ok(()) => trace!("Relaying login packet to client for {}", name),
                Err(e) => return error!("Failed to relay login packet to client {}: {}", name, e),
            }
            client_conn.set_state(State::Play);
            conn.set_state(State::Play);
        },
        Ok(Some(packet)) => return error!("Expected a login success packet for {}, got {:?}", name, packet),
        Ok(None) => return info!("Client connection closed before receiving login packet {}", name),
        Err(e) => return error!("Failed to read packet from client for {}: {}", name, e),
    }
    let (mut server_reader, mut server_writer) = client_conn.into_split(); // proxy's connection to the server
    let (mut client_reader, mut client_writer) = conn.into_split(); // proxy's connection to the client
    let is_alive_arc = Arc::new(RwLock::new(true));
    {
        let is_alive_arc= is_alive_arc.clone();
        let name = name.clone();
        thread::spawn(move || {
            // pass data along from client to server
            while *is_alive_arc.read().unwrap() {
                match client_reader.read_raw_packet::<RawPacketLatest>() {
                    Ok(Some(raw_packet)) => match server_writer.write_raw_packet(raw_packet) {
                        Ok(()) => {}, // trace!("Relaying raw packet client to server for {}", name),
                        Err(e) => error!("Failed to relay packet client to server for {}: {}", name, e),
                    },
                    Ok(None) => {
                        info!("Client connection closed for {}", name);
                        break;
                    },
                    Err(e) => {
                        // not sure if we should be doing something here
                        error!("Failed to read packet client to server for {}: {}", name, e);
                    },
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
                    Ok(()) => {}, //trace!("Relaying raw packet server to clientfor {}", name),
                    Err(e) => error!("Failed to relay packet server to client for {}: {}", name, e),
                }
            },
            Ok(None) => {
                info!("Server connection closed for {}", name);
                break;
            },
            Err(e) => {
                // not sure if we should be doing something here
                error!("Failed to read packet server to client for {}: {}", name, e);
            },
        };
    }
    *is_alive_arc.write().unwrap() = false;
    trace!("server to client thread closed for {}", name);
}
