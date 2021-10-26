use std::sync::Arc;

use anyhow::Context;

use crate::{
    commands::{
        CommandSender,
        SplinterCommand,
    },
    proxy::SplinterProxy,
};

inventory::submit! {
    SplinterCommand {
        name: "switch",
        action: Box::new(|proxy: &Arc<SplinterProxy>, _cmd: &str, args: &[&str], sender: &CommandSender| {
            if args.len() == 0 || args.len() > 2 {
                bail!("Invalid number of arguments");
            }
            let destination = args[0].parse::<u64>().with_context(|| "Failed to read target server id")?;
            if args.len() == 2 {
                let player_map = smol::block_on(proxy.players.read());
                let client = player_map.get(args[1]).ok_or_else(|| anyhow!("Failed to find player"))?;
                    smol::block_on(client.swap_dummy(destination))?;
            }
            else if args.len() == 1{
                if let CommandSender::Player(cl) = sender {
                    smol::block_on(cl.swap_dummy(destination))?;
                }
                else {
                    bail!("Command sender is not player; cannot infer player");
                }
            }
            Ok(())
        }),
    }
}
inventory::submit! {
    SplinterCommand {
        name: "dummyjoin",
        action: Box::new(|proxy: &Arc<SplinterProxy>, _cmd: &str, args: &[&str], sender: &CommandSender| {
            if args.len() == 0 || args.len() > 2 {
                bail!("Invalid number of arguments");
            }
            let target_id = args[0].parse::<u64>().with_context(|| "Invalid target server id")?;
            if args.len() == 2 {
                let player_map = smol::block_on(proxy.players.read());
                let client = player_map.get(args[1]).ok_or_else(|| anyhow!("Failed to find player"))?;
                smol::block_on(client.connect_dummy(target_id))?;
            }
            else if args.len() == 1 {
                if let CommandSender::Player(cl) = sender {
                    smol::block_on(cl.connect_dummy(target_id))?;
                }
                else {
                    bail!("Command sender is not a player; cannot infer player");
                }
            }
            Ok(())
        }),
    }
}
