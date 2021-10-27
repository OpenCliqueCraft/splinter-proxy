use std::sync::Arc;

use crate::{
    commands::{
        CommandSender,
        SplinterCommand,
    },
    proxy::SplinterProxy,
};
inventory::submit! {
    SplinterCommand {
        name: "list",
        action: Box::new(|proxy: &Arc<SplinterProxy>, _cmd: &str, _args: &[&str], sender: &CommandSender| {
            let players = smol::block_on(proxy.players.read());
            let msg = format!(
                "{}/{} players: {}",
                players.len(),
                match proxy.config.max_players {
                    Some(players) => players.to_string(),
                    None => "--".into(),
                },
                players
                    .iter()
                    .map(|(name, _)| name.to_owned())
                    .reduce(|a, b| format!("{}, {}", a, b))
                    .unwrap_or_else(String::new),
            );
            if let Err(e) = sender.respond_sync(msg) {
                error!(
                    "Failed to send player list response to {}: {}",
                    sender.name(),
                    e
                );
            }
            Ok(())
        }),
    }
}
