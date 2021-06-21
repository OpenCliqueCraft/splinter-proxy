use std::{
    collections::HashMap,
    net::{
        SocketAddr,
        TcpListener,
    },
    str::FromStr,
    sync::{
        Arc,
        RwLock,
    },
};

use mcproto_rs::{
    protocol::RawPacket,
    v1_16_3::RawPacket753,
};
use smol::{
    lock::Mutex,
    Async,
};

use crate::{
    client::{
        self,
        SplinterClient,
    },
    config::SplinterConfig,
    mapping::SplinterMapping,
    protocol::{
        version,
        ProtocolVersion,
        Tags,
    },
    server::SplinterServer,
};

pub struct SplinterProxy {
    pub protocol: ProtocolVersion,
    pub alive: RwLock<bool>,
    pub config: SplinterConfig,
    pub players: RwLock<HashMap<String, Arc<SplinterClient<version::V753>>>>, // TODO
    pub servers: RwLock<HashMap<u64, Arc<SplinterServer>>>,
    pub mapping: Mutex<SplinterMapping>,
    pub tags: Mutex<Option<Tags>>,
}

impl SplinterProxy {
    pub fn new(config: SplinterConfig) -> anyhow::Result<Self> {
        let servers = {
            let mut map = HashMap::new();
            for (id, addr_str) in config.simulation_servers.iter() {
                map.insert(
                    *id,
                    Arc::new(SplinterServer {
                        id: *id,
                        address: SocketAddr::from_str(addr_str)?,
                    }),
                );
            }
            RwLock::new(map)
        };
        Ok(Self {
            protocol: ProtocolVersion::from_number(config.protocol)?,
            alive: RwLock::new(true),
            config,
            players: RwLock::new(HashMap::new()),
            servers,
            mapping: Mutex::new(SplinterMapping::new()),
            tags: Mutex::new(None),
        })
    }
    pub fn is_alive(&self) -> bool {
        *self.alive.read().unwrap()
    }
}
pub async fn run(proxy: Arc<SplinterProxy>) -> anyhow::Result<()> {
    let address = SocketAddr::from_str(proxy.config.proxy_address.as_str())?;
    let listener = Async::<TcpListener>::bind(address)?;
    {
        let proxy = Arc::clone(&proxy);
        smol::spawn(async move {
            info!("Listening for incoming connections on {}", address);
            loop {
                let (stream, addr) = match listener.accept().await {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to accept a client: {}", e);
                        continue;
                    }
                };
                if let Err(e) = client::handle(stream, addr, Arc::clone(&proxy)) {
                    error!("Failed to handle connection from {}: {}", addr, e);
                }
            }
        })
        .detach();
    }
    loop {
        if !proxy.is_alive() {
            break;
        }
    }
    Ok(())
}
