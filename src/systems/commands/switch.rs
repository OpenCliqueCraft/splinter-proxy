use std::sync::Arc;

use anyhow::Context;

use crate::{
    protocol::v_cur::send_position_set,
    proxy::SplinterProxy,
    systems::commands::{
        CommandSender,
        SplinterCommand,
    },
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
                "list" => {
                    info!("List of connected dummies: {}", client.dummy_servers.load().iter().map(|(id, _)| format!("{}", id)).reduce(|a, b| format!("{}, {}", a, b)).unwrap_or_else(|| String::from("None")));
                },
                _ => bail!("Unknown subcommand"),
            }
            Ok(())
        }),
    }
}

// command for testing manually sending the position set packet
inventory::submit! {
    SplinterCommand {
        name: "send",
        action: Box::new(|proxy: &Arc<SplinterProxy>, _cmd: &str, args: &[&str], _sender: &CommandSender| {
            let player_map = smol::block_on(proxy.players.read());
            let client = player_map.get(args[0]).ok_or_else(|| anyhow!("Failed to find player"))?;
            //let target_id = args[1].parse::<u64>().with_context(|| "Invalid target server id")?;
            let active_server = client.active_server.load();
            smol::block_on(async {
                send_position_set(&mut *active_server.writer.lock().await, 0., 20., 0.).await
            })?;
            Ok(())
        })
    }
}
