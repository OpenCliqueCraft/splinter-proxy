use std::{
    io,
    sync::Arc,
};

use blocking::{
    unblock,
    Unblock,
};
use mcproto_rs::uuid::UUID4;
use smol;

use crate::{
    chat::ToChat,
    client::SplinterClient,
    init::SplinterSystem,
    proxy::SplinterProxy,
};

mod kick;
mod list;
mod stop;

pub enum CommandSender {
    Player(Arc<SplinterClient>),
    Console,
}

impl CommandSender {
    pub async fn respond(&self, msg: impl ToChat + ToString) -> anyhow::Result<()> {
        match self {
            CommandSender::Player(client) => client.send_message(msg, self).await,
            CommandSender::Console => {
                info!("{}", msg.to_string());
                Ok(())
            }
        }
    }
    pub fn name(&self) -> String {
        match self {
            CommandSender::Player(client) => client.name.to_owned(),
            CommandSender::Console => "console".into(),
        }
    }
    pub fn uuid(&self) -> UUID4 {
        match self {
            CommandSender::Player(client) => client.uuid,
            CommandSender::Console => UUID4::from(0u128),
        }
    }
}

impl Clone for CommandSender {
    fn clone(&self) -> Self {
        match self {
            Self::Console => Self::Console,
            Self::Player(client) => Self::Player(Arc::clone(client)),
        }
    }
}

pub type CommandFn = Box<
    dyn Send + Sync + Fn(&Arc<SplinterProxy>, &str, &[&str], &CommandSender) -> anyhow::Result<()>,
>;
pub struct SplinterCommand {
    pub name: &'static str,
    pub action: CommandFn,
}

inventory::collect!(SplinterCommand);

pub async fn process_command(
    proxy: &Arc<SplinterProxy>,
    cmd: &str,
    args: &[&str],
    sender: &CommandSender,
) -> anyhow::Result<()> {
    if let Some(cmd_data) = inventory::iter::<SplinterCommand>
        .into_iter()
        .find(|cmd_data| cmd_data.name.eq(cmd))
    {
        (cmd_data.action)(proxy, cmd, args, sender)?;
    } else {
        bail!("Unknown command \"{}\"", cmd);
    }
    Ok(())
}

inventory::submit! {
    SplinterSystem {
        name: "Console Command Listener",
        init: Box::new(|proxy| {
            Box::pin(init(proxy))
        })
    }
}

async fn init(proxy: Arc<SplinterProxy>) -> anyhow::Result<()> {
    let mut stdin = Unblock::new(unblock(io::stdin).await);
    smol::spawn(async move {
        loop {
            let line = match stdin
                .with_mut(|stdin| {
                    let mut line = String::new();
                    match stdin.read_line(&mut line) {
                        Ok(_) => Ok(line),
                        Err(e) => Err(e),
                    }
                })
                .await
            {
                Ok(line) => line,
                Err(e) => {
                    error!("Failed to read line from stdin: {}", e);
                    break;
                }
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut split = line.split_whitespace();
            let cmd = split.next().unwrap(); // at this point, something is in the command
            let args = split.collect::<Vec<&str>>();
            let sender = CommandSender::Console;
            if let Err(e) = process_command(&proxy, cmd, args.as_slice(), &sender).await {
                if let Err(e) = sender.respond(format!("Command failed: {:?}", e)).await {
                    error!(
                        "Failed to send command failure message to {}: {}",
                        sender.name(),
                        e
                    );
                }
            }
        }
    })
    .detach();
    Ok(())
}
