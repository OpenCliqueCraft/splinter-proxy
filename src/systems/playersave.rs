use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};
use smol::Timer;

use crate::{protocol::current::uuid::UUID4, proxy::SplinterProxy, systems::SplinterSystem};

inventory::submit! {
    SplinterSystem {
        name: "Player Saver",
        init: Box::new(|proxy| {
            Box::pin(async move {
                smol::spawn(async move {
                    if let Err(e) = player_save_loop(proxy).await {
                        error!("Player Saver encountered an error: {:?}", e);
                    }
                }).detach();
                Ok(())
            })
        }),
    }
}

pub const PLAYER_DATA_FILENAME: &str = "./playerdata.ron";
pub const DEFAULT_SPAWN_POSITION: (f64, f64, f64) = (0., 8., 0.);

#[derive(Debug, Deserialize, Serialize)]
pub struct PlInfoPlayer {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub name: String,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct PlInfo {
    pub players: HashMap<UUID4, PlInfoPlayer>,
}
impl Default for PlInfo {
    fn default() -> PlInfo {
        PlInfo {
            players: HashMap::new(),
        }
    }
}

pub async fn player_save_loop(proxy: Arc<SplinterProxy>) -> anyhow::Result<()> {
    loop {
        if !proxy.alive.load(Ordering::Relaxed) {
            break;
        }
        Timer::after(Duration::from_secs(30)).await;
        if let Err(e) = save_player_data(&*proxy.player_data.lock().await, PLAYER_DATA_FILENAME) {
            error!("Player Saver error when reading file: {:?}", e);
        }
    }
    Ok(())
}
pub fn load_player_data(filename: impl AsRef<str>) -> anyhow::Result<PlInfo> {
    let existing_file = fs::read_to_string(filename.as_ref())?;
    let existing_plinfo: PlInfo = ron::de::from_str(&existing_file)?;
    Ok(existing_plinfo)
}
pub fn save_player_data(info: &PlInfo, filename: impl AsRef<str>) -> anyhow::Result<()> {
    debug!("saving player data...");
    File::create(filename.as_ref())?
        .write_all(ron::ser::to_string_pretty(info, PrettyConfig::default())?.as_bytes())
        .map_err(anyhow::Error::new)
}
