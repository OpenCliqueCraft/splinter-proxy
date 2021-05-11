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

#[derive(Serialize, Deserialize)]
pub struct SplinterProxyConfiguration {
    pub protocol_version: i32,
    pub version: Option<(String, i32)>,
    pub server_address: String,
    pub bind_address: String,
    pub max_players: Option<u32>,
    pub status: SplinterProxyStatus,
}

#[derive(Serialize, Deserialize)]
pub struct SplinterProxyStatus {
    pub player_count: Option<u32>,
    pub player_sample: Option<Vec<(String, String)>>,
    pub motd: String, // TextComponent,
}

impl Default for SplinterProxyConfiguration {
    fn default() -> Self {
        SplinterProxyConfiguration {
            protocol_version: 753,
            version: Some(("Splinter 1.16.3".into(), 753)),
            server_address: "127.0.0.1:25400".into(),
            bind_address: "127.0.0.1:25565".into(),
            max_players: None,
            status: SplinterProxyStatus::default(),
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

pub enum ConfigLoadError {
    NoFile,
    Io(io::Error),
    De(ron::Error),
}

pub enum ConfigSaveError {
    Create(io::Error),
    Write(ron::Error),
}

impl SplinterProxyConfiguration {
    pub fn load(filepath: &Path) -> Result<SplinterProxyConfiguration, ConfigLoadError> {
        if !filepath.is_file() {
            return Err(ConfigLoadError::NoFile);
        }
        let data = read_to_string(filepath).map_err(|e| ConfigLoadError::Io(e))?;
        from_str(data.as_str()).map_err(|e| ConfigLoadError::De(e))
    }
    pub fn save(&self, filepath: &Path) -> Result<(), ConfigSaveError> {
        let file = File::create(&filepath).map_err(|e| ConfigSaveError::Create(e))?;
        ser::to_writer_pretty(file, self, PrettyConfig::default())
            .map_err(|e| ConfigSaveError::Write(e))
    }
    pub fn server_status(&self, total_players: Option<u32>) -> StatusSpec {
        StatusSpec {
            version: self
                .version
                .as_ref()
                .map(|(name, protocol)| StatusVersionSpec {
                    name: name.clone(),
                    protocol: *protocol,
                }),
            players: StatusPlayersSpec {
                max: self
                    .max_players
                    .unwrap_or_else(|| total_players.unwrap_or(0) + 1) as i32,
                online: total_players.unwrap_or(0) as i32,
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
                    .unwrap_or_else(
                        || vec![], // should put the actual players of the server here
                    ),
            },
            description: Chat::Text(TextComponent {
                text: self.status.motd.clone(),
                base: BaseComponent::default(),
            }), // Chat::Text(self.status.motd.clone()),
            favicon: None,
        }
    }
}
