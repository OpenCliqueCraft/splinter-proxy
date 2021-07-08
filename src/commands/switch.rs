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
            if args.len() != 2 {
                bail!("Expected 2 arguments");
            }
            let player_name = args[0];
            let destination = args[1].parse::<u64>().with_context(|| "Failed to read destination server id from second argument")?;
            todo!();
            Ok(())
        }),
    }
}
