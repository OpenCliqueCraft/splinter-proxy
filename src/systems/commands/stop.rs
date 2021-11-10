use std::sync::Arc;

use crate::{
    proxy::SplinterProxy,
    systems::commands::{CommandSender, SplinterCommand},
};
inventory::submit! {
    SplinterCommand {
        name: "stop",
        action: Box::new(|proxy: &Arc<SplinterProxy>, _cmd: &str, _args: &[&str], _sender: &CommandSender| {
            smol::block_on(proxy.shutdown());
            Ok(())
        }),
    }
}
