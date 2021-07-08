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
        action: Box::new(|proxy: &Arc<SplinterProxy>, _cmd: &str, _args: &[&str], _sender: &CommandSender| {
            let names = smol::block_on(proxy.players.read()).iter().map(|(name, _)| name.to_owned()).collect::<Vec<String>>();
            if !names.is_empty() {
                info!("Disconnecting clients");
                for name in names {
                    if let Err(e) = smol::block_on(proxy.kick_client(&name, ClientKickReason::Shutdown)) {
                        error!("Error kicking player \"{}\": {}", &name, e);
                    }
                }
            }
            info!("Shutting down");
            proxy.alive.store(Arc::new(false));
            Ok(())
        }),
    }
}
