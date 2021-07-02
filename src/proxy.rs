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
    time::Duration,
};

use mcproto_rs::{
    protocol::RawPacket,
    v1_16_3::RawPacket753,
};
use smol::{
    lock::Mutex,
    Async,
    Timer,
};

use crate::{
    client::{
        self,
        ClientVersion,
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
    pub players: RwLock<HashMap<String, Arc<SplinterClient>>>,
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
    pub async fn kick_client(
        &self,
        client_name: impl AsRef<str>,
        reason: ClientKickReason,
    ) -> anyhow::Result<()> {
        let name_string = client_name.as_ref().to_owned();
        let cl_opt = self
            .players
            .read()
            .unwrap()
            .get(&name_string)
            .map(Arc::clone);
        if let Some(client) = cl_opt {
            client.send_kick(reason).await?;
            client.set_alive(false).await;
            self.players.write().unwrap().remove(&name_string);
        } else {
            bail!("Failed to find client by the name \"{}\"", name_string);
        }
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
        Timer::after(Duration::from_secs(1)).await; // sleep so we're not constantly taking up a thread just for this
    }
    Ok(())
}
