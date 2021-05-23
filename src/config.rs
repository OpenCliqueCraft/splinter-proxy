use std::{
    default::Default,
    fs::{
        read_to_string,
        File,
    },
    io,
    path::Path,
};

use mcproto_rs::{
    status::{
        StatusPlayerSampleSpec,
        StatusPlayersSpec,
        StatusSpec,
        StatusVersionSpec,
    },
    types::{
        BaseComponent,
        Chat,
        TextComponent,
    },
    uuid::UUID4,
};
use ron::{
    self,
    de::from_str,
    ser::{
        self,
        PrettyConfig,
    },
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::state::SplinterState;

/// Structure containing the configuration of the proxy
///
/// A version with defaults can be obtained using this struct's [`Default`] implementation
#[derive(Serialize, Deserialize)]
pub struct SplinterProxyConfiguration {
    pub protocol_version: i32,

    /// [`Some`] tuple of the version name and protocol version number or [`None`] for unspecified
    ///
    /// `Some(("1.16.3", 754))` by default
    pub version: Option<(String, i32)>,

    /// The server address to proxy to
    ///
    /// `"127.0.0.1:25400"` by default
    pub server_address: String,

    /// The address for the proxy to bind to
    ///
    /// `"127.0.0.1:25565"` by default
    pub bind_address: String,

    /// [`Some`] number of max players for the proxy, or [`None`] for no limit and what is
    /// displayed is the number of players on the proxy plus one
    ///
    /// `None` by default
    pub max_players: Option<u32>,

    /// Information specific to proxy status requests
    ///
    /// Defaults in [`SplinterProxyStatus`]
    pub status: SplinterProxyStatus,

    /// Compression threshold for packets, equivalent to network-compression-threshold in
    /// Minecraft's
    /// [server.properties](https://minecraft.fandom.com/wiki/Server.properties). [`None`] disables
    /// compression, and [`Some`] enables compression with a threshold value.
    ///
    /// `Some(256)` by default
    pub compression_threshold: Option<i32>,
}

/// Information specific to proxy status requests
#[derive(Serialize, Deserialize)]
pub struct SplinterProxyStatus {
    /// [`Some`] with a number to specific a constant player count, or [`None`] for the actual
    /// number of players connected to the proxy
    ///
    /// `None` by default
    pub player_count: Option<u32>,

    /// [`Some`] with a list of name and UUID pairs for each player on the server, or [`None`] to
    /// let the proxy to generate the list of players connected
    ///
    /// `Some([])` by default
    pub player_sample: Option<Vec<(String, String)>>,

    /// The status text for the client's human to read
    ///
    /// `"Splinter Proxy"` by default
    pub motd: String, // TextComponent,
}

impl Default for SplinterProxyConfiguration {
    fn default() -> Self {
        SplinterProxyConfiguration {
            protocol_version: 754,
            version: Some(("Splinter 1.16.3".into(), 754)),
            server_address: "127.0.0.1:25400".into(),
            bind_address: "127.0.0.1:25565".into(),
            max_players: None,
            status: SplinterProxyStatus::default(),
            compression_threshold: Some(256),
        }
    }
}

impl Default for SplinterProxyStatus {
    fn default() -> Self {
        SplinterProxyStatus {
            player_count: None,
            player_sample: Some(vec![]),
            motd: "Splinter Proxy".into(), /* TextComponent {
                                            * text: "Splinter Proxy".into(),
                                            * base: BaseComponent::default(),
                                            * }, */
        }
    }
}

/// An error during config file loading
pub enum ConfigLoadError {
    /// No file has been found to load from
    NoFile,

    /// An [`io::Error`] occurred during load
    Io(io::Error),

    /// A [`ron::Error`] occurred during load
    De(ron::Error),
}

/// An error during config file saving
pub enum ConfigSaveError {
    /// [`io:Error`] during file creation
    Create(io::Error),

    /// [`ron::Error`] during writing config data
    Write(ron::Error),
}

impl SplinterProxyConfiguration {
    /// Loads [`SplinterProxyConfiguration`] from `filepath`
    pub fn load(filepath: &Path) -> Result<SplinterProxyConfiguration, ConfigLoadError> {
        if !filepath.is_file() {
            return Err(ConfigLoadError::NoFile);
        }
        let data = read_to_string(filepath).map_err(|e| ConfigLoadError::Io(e))?;
        from_str(data.as_str()).map_err(|e| ConfigLoadError::De(e))
    }
    /// Saves this [`SplinterProxyConfiguration`] to `filepath`
    pub fn save(&self, filepath: &Path) -> Result<(), ConfigSaveError> {
        let file = File::create(&filepath).map_err(|e| ConfigSaveError::Create(e))?;
        ser::to_writer_pretty(file, self, PrettyConfig::default())
            .map_err(|e| ConfigSaveError::Write(e))
    }

    /// Create the server status based on the proxy configuration
    pub fn server_status(&self, state: &SplinterState) -> StatusSpec {
        let total_players = state.players.read().unwrap().len();
        StatusSpec {
            version: self
                .version
                .as_ref()
                .map(|(name, protocol)| StatusVersionSpec {
                    name: name.clone(),
                    protocol: *protocol,
                }),
            players: StatusPlayersSpec {
                max: self.max_players.unwrap_or_else(|| total_players as u32 + 1) as i32,
                online: total_players as i32,
                sample: self
                    .status
                    .player_sample
                    .as_ref()
                    .map(|samples| {
                        samples
                            .iter()
                            .map(|(name, id)| StatusPlayerSampleSpec {
                                name: name.clone(),
                                id: UUID4::parse(id).unwrap_or(UUID4::random()),
                            })
                            .collect::<Vec<StatusPlayerSampleSpec>>()
                    })
                    .unwrap_or_else(|| {
                        state
                            .players
                            .read()
                            .unwrap()
                            .iter()
                            .map(|(_id, client)| StatusPlayerSampleSpec {
                                name: client.name.clone(),
                                id: client.uuid,
                            })
                            .collect::<Vec<StatusPlayerSampleSpec>>()
                    }),
            },
            description: Chat::Text(TextComponent {
                text: self.status.motd.clone(),
                base: BaseComponent::default(),
            }), // Chat::Text(self.status.motd.clone()),
            favicon: None,
        }
    }
}

/// Loads configuration from the specified .ron file
///
/// # Example
/// ```rust
/// let config = get_config("./config.ron");
/// if let Some(threshold) = config.compression_threshold {
///     info!("compression threshold is {}", threshold);
/// }
/// ```
pub fn get_config(config_path: &str) -> SplinterProxyConfiguration {
    let config = match SplinterProxyConfiguration::load(Path::new(config_path)) {
        Ok(config) => {
            info!("Config loaded from {}", config_path);
            config
        }
        Err(ConfigLoadError::NoFile) => {
            warn!(
                "No config file found at {}. Creating a new one from defaults",
                config_path
            );
            let config = SplinterProxyConfiguration::default();
            match config.save(Path::new(config_path)) {
                Ok(()) => {}
                Err(ConfigSaveError::Create(e)) => {
                    error!("Failed to create file at {}: {}", config_path, e);
                }
                Err(ConfigSaveError::Write(e)) => {
                    error!("Failed to write to {}: {}", config_path, e);
                }
            }
            config
        }
        Err(ConfigLoadError::Io(e)) => {
            error!(
                "Failed to read config file at {}: {} Using default settings",
                config_path, e
            );
            SplinterProxyConfiguration::default()
        }
        Err(ConfigLoadError::De(e)) => {
            error!(
                "Failure to deserialize config file at {}: {}. Using default settings",
                config_path, e
            );
            SplinterProxyConfiguration::default()
        }
    };

    config
}
