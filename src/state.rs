use std::{
    self,
    collections::HashMap,
    net::{
        SocketAddr,
        TcpStream,
    },
    sync::{
        Arc,
        Mutex,
        RwLock,
    },
};

use craftio_rs::CraftWriter;
use mcproto_rs::uuid::UUID4;

use crate::{
    config::SplinterProxyConfiguration,
    mapping::{
        ClientPacketMapFn,
        PacketMap,
        ServerPacketMapFn,
    },
};

/// Global state for the splinter proxy
pub struct SplinterState {
    /// Configuration
    pub config: RwLock<SplinterProxyConfiguration>,
    /// List of client states
    pub players: RwLock<HashMap<u64, Arc<SplinterClient>>>,
    /// List of servers
    pub servers: RwLock<HashMap<u64, SplinterServer>>,
    /// Client-proxy packet map
    pub client_packet_map: PacketMap<ClientPacketMapFn>,
    /// Proxy-server packet map
    pub server_packet_map: PacketMap<ServerPacketMapFn>,
}

/// Server state
pub struct SplinterServer {
    /// Internal unique ID of the server
    pub id: u64,
    /// Server address
    pub addr: SocketAddr,
}

/// Client state
pub struct SplinterClient {
    /// Internal unique ID of the client
    pub id: u64,
    /// Username of the client
    pub name: String,
    /// List of connections to servers
    pub servers: RwLock<HashMap<u64, Arc<SplinterServerConnection>>>,
    /// Writer to the client
    pub writer: Mutex<CraftWriter<TcpStream>>,
    /// Proxy's UUID of the client
    pub uuid: UUID4,
    /// Whether the client connection is alive
    pub alive: RwLock<bool>,
}

/// Server connection state specific to client-proxy-server
pub struct SplinterServerConnection {
    /// Internal unique ID of the server
    pub id: u64,
    /// Address of the server
    pub addr: SocketAddr,
    /// Writer to the server
    pub writer: Mutex<CraftWriter<TcpStream>>,
    /// Server's UUID for the client
    pub client_uuid: UUID4,
}

// impl SplinterServerConnection {
// pub fn server<'a>(&self, state: &'a SplinterState) -> &'a SplinterServer {
// state.servers.read().unwrap().get(&self.id).unwrap()
// }
// }

impl SplinterState {
    /// Creates a new splinter state given the proxy configuration
    pub fn new(config: SplinterProxyConfiguration) -> SplinterState {
        SplinterState {
            config: RwLock::new(config),
            players: RwLock::new(HashMap::new()),
            servers: RwLock::new(HashMap::new()),
            client_packet_map: HashMap::new(),
            server_packet_map: HashMap::new(),
        }
    }
    pub fn next_server_id(&self) -> u64 {
        // unlikely made for multithreading
        let mut id = 0u64;
        let lock = self.servers.read().unwrap();
        while lock.contains_key(&id) {
            id += 1;
        }
        return id;
    }
    pub fn next_client_id(&self) -> u64 {
        let mut id = 0u64;
        let lock = self.players.read().unwrap();
        while lock.contains_key(&id) {
            id += 1;
        }
        return id;
    }
}
