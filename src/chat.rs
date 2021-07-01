use std::sync::Arc;

use mcproto_rs::types::{
    BaseComponent,
    Chat,
    ColorCode,
    TextComponent,
};

use crate::{
    commands::CommandSender,
    protocol::PacketSender,
    proxy::SplinterProxy,
};

pub trait ToChat {
    fn to_chat(&self) -> Chat;
}
impl ToChat for Chat {
    fn to_chat(&self) -> Chat {
        self.clone()
    }
}
impl ToChat for &str {
    fn to_chat(&self) -> Chat {
        Chat::from_text(self)
    }
}
impl ToChat for String {
    fn to_chat(&self) -> Chat {
        Chat::from_text(self.as_str())
    }
}
pub fn format_chat_message_string(
    sender: &CommandSender,
    message: impl ToChat + ToString,
) -> String {
    format!("{}: {}", sender.name(), message.to_string(),)
}
pub fn format_chat_message(sender: &CommandSender, message: impl ToChat + ToString) -> Chat {
    Chat::Text(TextComponent {
        text: format!("{}", sender.name()),
        base: BaseComponent {
            bold: false,
            italic: false,
            underlined: false,
            strikethrough: false,
            obfuscated: false,
            color: Some(ColorCode::Blue),
            insertion: None,
            click_event: None,
            hover_event: None,
            extra: vec![Box::new(Chat::Text(TextComponent {
                text: ": ".into(),
                base: BaseComponent {
                    bold: false,
                    italic: false,
                    underlined: false,
                    strikethrough: false,
                    obfuscated: false,
                    color: Some(ColorCode::White),
                    insertion: None,
                    click_event: None,
                    hover_event: None,
                    extra: vec![Box::new(message.to_chat())],
                },
            }))],
        },
    })
}

pub async fn receive_chat_message<'a>(
    proxy: &Arc<SplinterProxy>,
    sender: &PacketSender<'a>,
    msg: &str,
) {
    if msg.is_empty() {
        return;
    }
    let client = match sender {
        PacketSender::Proxy(client) => client,
        PacketSender::Server(_) => return,
    };
    let cmd_sender = CommandSender::Player(Arc::clone(&client));
    let msg_string = format_chat_message_string(&cmd_sender, msg);
    info!("{}", msg_string);
    if let Some('/') = msg.chars().nth(0) {
        let server_id = client.active_server_id();
        if let Err(e) = client.relay_message(msg, server_id).await {
            error!(
                "Failed to relay chat message from \"{}\" to server \"{}\": {}",
                client.name(),
                server_id,
                e
            );
        }
    } else {
        let msg_chat = format_chat_message(&cmd_sender, msg);
        broadcast_message(proxy, &cmd_sender, msg_chat).await;
    }
}

pub async fn broadcast_message(
    proxy: &Arc<SplinterProxy>,
    sender: &CommandSender,
    msg: impl ToChat + Clone,
) {
    for (_, target) in proxy.players.read().unwrap().iter() {
        if let Err(e) = target.send_message(msg.clone(), sender).await {
            error!(
                "Failed to send broadcast message to {}: {}",
                &target.name(),
                e
            );
        }
    }
}
