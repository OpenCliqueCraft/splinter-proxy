use std::{
    pin::Pin,
    sync::Arc,
};

use smol::prelude::Future;

use crate::proxy::SplinterProxy;

pub mod commands;
pub mod eidautoremoval;
pub mod keepalive;
pub mod playersave;
pub mod zoning;

pub type SystemInitFn = Box<
    dyn Send + Sync + Fn(Arc<SplinterProxy>) -> Pin<Box<dyn Future<Output = anyhow::Result<()>>>>,
>;
pub struct SplinterSystem {
    pub name: &'static str,
    pub init: SystemInitFn,
}
inventory::collect!(SplinterSystem);

pub async fn init(proxy: &Arc<SplinterProxy>) -> anyhow::Result<()> {
    for system in inventory::iter::<SplinterSystem> {
        info!("Starting system: {}", system.name);
        (system.init)(Arc::clone(proxy)).await?;
    }
    Ok(())
}
