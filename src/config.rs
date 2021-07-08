use std::{
    fs::{
        self,
        File,
    },
    io::Write,
    path::Path,
};

use mcproto_rs::{
    status::{
        StatusPlayerSampleSpec,
        StatusPlayersSpec,
        StatusSpec,
        StatusVersionSpec,
    },
    types::Chat,
};
use ron::ser::PrettyConfig;
use serde::{
    Deserialize,
    Serialize,
};

use crate::proxy::SplinterProxy;

#[derive(Clone, Serialize, Deserialize)]
pub struct SplinterConfig {
    pub protocol: i32,
    pub display_version: Option<String>,
    pub simulation_servers: Vec<(u64, String)>,
    pub proxy_address: String,
    pub max_players: Option<i32>,
    pub motd: String,
    pub compression_threshold: Option<i32>,
    pub improper_version_disconnect_message: String,
    pub brand: String,
}
impl Default for SplinterConfig {
    fn default() -> Self {
        Self {
            protocol: 754,
            display_version: Some("Splinter 1.16.5".into()),
            simulation_servers: vec![(0, "127.0.0.1:25400".into())],
            proxy_address: "127.0.0.1:25565".into(),
            max_players: None,
            motd: "Splinter Proxy".into(),
            compression_threshold: Some(256),
            improper_version_disconnect_message: "Your client version is not supported".into(),
            brand: "Splinter".into(),
        }
    }
}

impl SplinterConfig {
    /// Attempts to read splinter config from a string
    pub fn from_str(data: impl AsRef<str>) -> anyhow::Result<SplinterConfig> {
        ron::de::from_str(data.as_ref()).map_err(anyhow::Error::new)
    }
    /// Attempts to read splinter config from a file
    pub fn from_file(filepath: impl AsRef<Path>) -> anyhow::Result<SplinterConfig> {
        Self::from_str(fs::read_to_string(filepath)?)
    }
    /// Attempts to convert this splinter config to a string
    pub fn to_string(&self) -> anyhow::Result<String> {
        ron::ser::to_string_pretty(self, PrettyConfig::default()).map_err(anyhow::Error::new)
    }
    /// Attempts to write this splinter config to a file
    pub fn to_file(&self, filepath: impl AsRef<Path>) -> anyhow::Result<()> {
        File::create(filepath)?
            .write_all(self.to_string()?.as_bytes())
            .map_err(anyhow::Error::new)
    }
    /// Gets the server status given the config and the proxy
    pub fn server_status(&self, proxy: &SplinterProxy) -> StatusSpec {
        let players = smol::block_on(proxy.players.read());
        let total_players = players.len();
        StatusSpec {
            version: self.display_version.as_ref().map(|name| StatusVersionSpec {
                name: name.clone(),
                protocol: self.protocol,
            }),
            players: StatusPlayersSpec {
                max: self.max_players.unwrap_or(total_players as i32 + 1),
                online: total_players as i32,
                sample: players
                    .iter()
                    .map(|(name, client)| StatusPlayerSampleSpec {
                        name: name.clone(),
                        id: client.uuid,
                    })
                    .collect::<Vec<StatusPlayerSampleSpec>>(),
            },
            description: Chat::from_text(self.motd.as_str()),
            favicon: None,
        }
    }
}
