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
        name: "dummy",
        action: Box::new(|proxy: &Arc<SplinterProxy>, _cmd: &str, args: &[&str], _sender: &CommandSender| {
            if args.len() != 3 {
                bail!("Invalid number of arguments");
            }
            let target_id = args[1].parse::<u64>().with_context(|| "Invalid target server id")?;
            let player_map = smol::block_on(proxy.players.read());
            let client = player_map.get(args[2]).ok_or_else(|| anyhow!("Failed to find player"))?;
            match args[0] {
                "switch" => {
                    smol::block_on(client.swap_dummy(target_id))?;
                },
                "join" => {
                    smol::block_on(client.connect_dummy(target_id))?;
                },
                "disconnect" => {
                    smol::block_on(client.disconnect_dummy(target_id))?;
                },
                _ => bail!("Unknown subcommand"),
            }
            Ok(())
        }),
    }
}
