use craftio_rs::CraftAsyncWriter;
use mcproto_rs::v1_16_3::{
    ChatPosition,
    Packet753,
    Packet753Kind,
    PlayClientChatMessageSpec,
    PlayServerChatMessageSpec,
};

use super::RelayPass;
use crate::{
    chat::{
        receive_chat_message,
        ToChat,
    },
    client::SplinterClient,
    commands::CommandSender,
    events::LazyDeserializedPacket,
    protocol::version::V753,
};

inventory::submit! {
    RelayPass(Box::new(|proxy, _connection, client, sender, lazy_packet, destination| {
        if lazy_packet.kind() == Packet753Kind::PlayClientChatMessage {
            match lazy_packet.packet() {
                Ok(Packet753::PlayClientChatMessage(body)) => smol::block_on(receive_chat_message(proxy, client, sender, &body.message)),
                Ok(_) => unreachable!(),
                Err(e) => {
                    error!("Failed to deserialize chat message: {}", e);
                }
            }
            *destination = None;
        }
    }))
}

impl SplinterClient {
    pub async fn send_message_v753(
        &self,
        msg: impl ToChat,
        sender: &CommandSender,
    ) -> anyhow::Result<()> {
        self.write_packet_v753(LazyDeserializedPacket::<V753>::from_packet(
            Packet753::PlayServerChatMessage(PlayServerChatMessageSpec {
                message: msg.to_chat(),
                position: match sender {
                    CommandSender::Player(_) => ChatPosition::ChatBox,
                    CommandSender::Console => ChatPosition::SystemMessage,
                },
                sender: sender.uuid(),
            }),
        ))
        .await
    }
    pub async fn relay_message_v753(&self, msg: &str) -> anyhow::Result<()> {
        self.active_server
            .load()
            .writer
            .lock()
            .await
            .write_packet_async(Packet753::PlayClientChatMessage(
                PlayClientChatMessageSpec {
                    message: msg.to_owned(),
                },
            ))
            .await
            .map_err(|e| e.into())
    }
}
