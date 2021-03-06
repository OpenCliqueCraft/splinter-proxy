// #![allow(unused_imports)]
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate simplelog;

use std::sync::Arc;

mod protocol;
mod proxy;
mod systems;

use crate::proxy::{
    config::SplinterConfig,
    logging as splinter_logging,
    SplinterProxy,
};

const CONFIG_FILENAME: &str = "./config.ron";

fn main() -> anyhow::Result<()> {
    splinter_logging::init()?;
    let config = match SplinterConfig::from_file(CONFIG_FILENAME) {
        Ok(config) => config,
        Err(e) => {
            warn!("Failed to read file at \"{}\": {}", CONFIG_FILENAME, e);
            SplinterConfig::default()
        }
    };
    if let Err(e) = config.to_file(CONFIG_FILENAME) {
        warn!("Failed to write config to \"{}\": {}", CONFIG_FILENAME, e);
    }
    info!("Loaded configuration");
    let proxy = SplinterProxy::new(config)?;
    let proxy_arc = Arc::new(proxy);
    info!("Starting Splinter Proxy");
    smol::block_on(async {
        if let Err(e) = systems::init(&proxy_arc).await {
            bail!("Failed to start proxy: {}", e);
        }
        proxy::run(proxy_arc).await
    })
}
