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
        action: Box::new(|proxy: &Arc<SplinterProxy>, cmd: &str, args: &[&str], sender: &CommandSender| {
            let players = proxy.players.read().unwrap();
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
                    .unwrap_or("".into()),
            );
            if let Err(e) = smol::block_on(sender.respond(msg)) {
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
