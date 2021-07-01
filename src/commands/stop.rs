use std::sync::Arc;

use crate::{
    commands::{
        CommandSender,
        SplinterCommand,
    },
    proxy::{
        ClientKickReason,
        SplinterProxy,
    },
};
inventory::submit! {
    SplinterCommand {
        name: "stop",
        action: Box::new(|proxy: &Arc<SplinterProxy>, cmd: &str, args: &[&str], sender: &CommandSender| {
            let names = proxy.players.read().unwrap().iter().map(|(name, _)| name.to_owned()).collect::<Vec<String>>();
            if !names.is_empty() {
                info!("Disconnecting clients");
                for name in names {
                    if let Err(e) = smol::block_on(proxy.kick_client(&name, ClientKickReason::Shutdown)) {
                        error!("Error kicking player \"{}\": {}", &name, e);
                    }
                }
            }
            info!("Shutting down");
            *proxy.alive.write().unwrap() = false;
            Ok(())
        }),
    }
}
