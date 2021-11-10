use std::sync::Arc;

use crate::{
    proxy::{
        ClientKickReason,
        SplinterProxy,
    },
    systems::commands::{
        CommandSender,
        SplinterCommand,
    },
};
inventory::submit! {
    SplinterCommand {
        name: "kick",
        action: Box::new(|proxy: &Arc<SplinterProxy>, _cmd: &str, args: &[&str], sender: &CommandSender| {
            if args.is_empty() {
                bail!("Expected at least one argument");
            }
            let mut arg_iter = args.iter();
            let name = arg_iter.next().unwrap();
            let message = if args.len() > 1 {
                Some(arg_iter.fold(String::new(), |mut acc, word| {
                    acc.push_str(word);
                    acc
                }))
            } else {
                None
            };
            smol::block_on(proxy.kick_client(name, ClientKickReason::Kicked(sender.name(), message)))?;
            Ok(())
        }),
    }
}
