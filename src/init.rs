use std::{
    pin::Pin,
    sync::Arc,
};

use smol::prelude::Future;

use crate::proxy::SplinterProxy;
pub struct SplinterSystem {
    pub name: &'static str,
    pub init: Box<
        dyn Send
            + Sync
            + Fn(Arc<SplinterProxy>) -> Pin<Box<dyn Future<Output = anyhow::Result<()>>>>,
    >,
}
inventory::collect!(SplinterSystem);

pub async fn init(proxy: &Arc<SplinterProxy>) -> anyhow::Result<()> {
    for system in inventory::iter::<SplinterSystem> {
        info!("Starting system: {}", system.name);
        (system.init)(Arc::clone(proxy)).await?;
    }
    Ok(())
}
