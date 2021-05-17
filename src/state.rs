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

use crate::config::SplinterProxyConfiguration;

/// Global state for the splinter proxy
pub struct SplinterState {
    /// Configuration
    pub config: RwLock<SplinterProxyConfiguration>,
    /// List of client states
    pub players: RwLock<HashMap<u64, Arc<SplinterClient>>>,
}

/// Client state
pub struct SplinterClient {
    /// Internal unique ID of the client
    pub id: u64,
    /// Username of the client
    pub name: String,
    /// List of connections to servers
    pub servers: RwLock<Vec<SplinterServer>>,
    /// Writer to the client
    pub writer: Mutex<CraftWriter<TcpStream>>,
    /// Proxy's UUID of the client
    pub uuid: UUID4,
}

/// Server state specific to client-proxy-server
pub struct SplinterServer {
    /// Internal unique ID of the server
    pub id: u64,
    /// Address of the server
    pub addr: SocketAddr,
    /// Writer to the server
    pub writer: Mutex<CraftWriter<TcpStream>>,
    /// Server's UUID for the client
    pub client_uuid: UUID4,
}

impl SplinterState {
    /// Creates a new splinter state given the proxy configuration
    pub fn new(config: SplinterProxyConfiguration) -> Arc<SplinterState> {
        Arc::new(SplinterState {
            config: RwLock::new(config),
            players: RwLock::new(HashMap::new()),
        })
    }
}
