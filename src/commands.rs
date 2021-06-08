use std::{
    io,
    sync::Arc,
    thread,
};

use craftio_rs::WriteError;
use mcproto_rs::{
    types::Chat,
    uuid::UUID4,
};

use crate::{
    chat::{
        send_message,
        IntoChat,
    },
    connection::{
        kick_client,
        ClientKickReason,
    },
    state::{
        SplinterClient,
        SplinterState,
    },
};

pub enum CommandSender {
    Player(Arc<SplinterClient>),
    Console,
}

impl CommandSender {
    pub fn respond(
        &self,
        msg: impl IntoChat + ToString,
        state: &Arc<SplinterState>,
    ) -> Result<(), WriteError> {
        match self {
            CommandSender::Player(client) => {
                send_message(state, &CommandSender::Console, &client, msg)
            }
            CommandSender::Console => {
                info!("{}", msg.to_string());
                Ok(())
            }
        }
    }
    pub fn name(&self) -> String {
        match self {
            CommandSender::Player(client) => client.name.clone(),
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

pub type SplinterCommandFn =
    Box<dyn Sync + Send + Fn(&Arc<SplinterState>, &CommandSender, &str, &[&str]) -> bool>;
pub struct SplinterCommand {
    pub name: String,
    pub action: SplinterCommandFn,
}

pub fn init(state: &mut SplinterState) {
    for cmd in [
        SplinterCommand {
            name: "say".into(),
            action: Box::new(|state, sender, _cmd, args| {
                let message = args
                    .iter()
                    .map(|val| val.to_string())
                    .reduce(|a, b| format!("{} {}", a, b))
                    .unwrap_or("".into());
                for (_id, target) in state.players.read().unwrap().iter() {
                    if let Err(e) = send_message(state, sender, target, message.as_str()) {
                        error!(
                            "Failed to send message from {} to {}: {}",
                            sender.name(),
                            target.name,
                            e,
                        );
                    }
                }
                true
            }),
        },
        SplinterCommand {
            name: "kick".into(),
            action: Box::new(|state, sender, _cmd, args| {
                let mut arg_iter = args.iter();
                if let Some(name) = arg_iter.next() {
                    if let Some(target_client) = state.find_client_by_name(name) {
                        kick_client(
                            &target_client,
                            state,
                            ClientKickReason::Kicked(
                                sender.name(),
                                arg_iter.next().map(|reason| String::from(*reason)),
                            ),
                        );
                        return true;
                    }
                }
                false
            }),
        },
        SplinterCommand {
            name: "help".into(),
            action: Box::new(|state, sender, _cmd, _args| {
                let msg = format!(
                    "List of commands: {}",
                    state
                        .commands
                        .iter()
                        .map(|(_, cmd)| cmd.name.clone())
                        .reduce(|a, b| format!("{}, {}", a, b))
                        .unwrap_or("".into()),
                );
                if let Err(e) = sender.respond(msg, state) {
                    error!(
                        "Failed to send help message response to {}: {}",
                        sender.name(),
                        e
                    );
                }
                true
            }),
        },
        SplinterCommand {
            name: "list".into(),
            action: Box::new(|state, sender, _cmd, _args| {
                let player_map = state.players.read().unwrap();
                let msg = format!(
                    "{}/{} players: {}",
                    player_map.len(),
                    match state.config.read().unwrap().max_players {
                        Some(players) => players.to_string(),
                        None => "--".into(),
                    },
                    player_map
                        .iter()
                        .map(|(_, client)| client.name.clone())
                        .reduce(|a, b| format!("{}, {}", a, b))
                        .unwrap_or("".into()),
                );
                if let Err(e) = sender.respond(msg, state) {
                    error!(
                        "Failed to send player list response to {}: {}",
                        sender.name(),
                        e
                    );
                }
                true
            }),
        },
        SplinterCommand {
            name: "stop".into(),
            action: Box::new(|state, _sender, _cmd, _args| {
                info!("Disconnecting clients");
                for (_, client) in state.players.read().unwrap().iter() {
                    kick_client(client, state, ClientKickReason::Shutdown);
                }
                info!("Shutting down");
                *state.alive.write().unwrap() = false;
                true
            }),
        },
    ] {
        state.commands.insert(cmd.name.clone(), cmd);
    }
}

pub fn init_post(state: Arc<SplinterState>) {
    thread::spawn(move || loop {
        let mut line = String::new();
        if let Err(e) = io::stdin().read_line(&mut line) {
            error!("Failed to read line from stdin: {}", e);
            break;
        }
        let line = line.trim();
        if line.len() == 0 {
            continue;
        }
        let mut split = line.split_whitespace();
        let cmd = split.next().unwrap(); // at this point, something is in the command
        let args = split.collect::<Vec<&str>>();
        if let Err(e) = process_command(&state, &CommandSender::Console, cmd, args.as_slice()) {
            error!("Command failed: {:?}", e);
        }
    });
}

#[derive(Debug)]
pub enum CommandProcessError {
    UnknownCommand,
}

pub fn process_command(
    state: &Arc<SplinterState>,
    sender: &CommandSender,
    cmd: &str,
    args: &[&str],
) -> Result<(), CommandProcessError> {
    if let Some(cmd_data) = state.commands.get(&String::from(cmd)) {
        (cmd_data.action)(state, sender, cmd, args);
        Ok(())
    } else {
        Err(CommandProcessError::UnknownCommand)
    }
}
