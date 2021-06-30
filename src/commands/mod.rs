use std::sync::Arc;

use mcproto_rs::uuid::UUID4;

use crate::{
    chat::ToChat,
    client::{
        SplinterClient,
        SplinterClientVersion,
    },
    events::LazyDeserializedPacket,
    mapping::SplinterMapping,
    protocol::{
        version::V753,
        PacketSender,
    },
    proxy::SplinterProxy,
};

pub enum CommandSender {
    Player(Arc<SplinterClientVersion>),
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
            CommandSender::Player(client) => client.name().to_owned(),
            CommandSender::Console => "console".into(),
        }
    }
    pub fn uuid(&self) -> UUID4 {
        match self {
            CommandSender::Player(client) => client.uuid(),
            CommandSender::Console => UUID4::from(0u128),
        }
    }
}

pub struct SplinterCommand {
    pub name: &'static str,
    pub action: Box<dyn Send + Sync + Fn(&Arc<SplinterProxy>, &str) -> anyhow::Result<()>>,
}

inventory::collect!(SplinterCommand);
