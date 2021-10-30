use craftio_rs::CraftAsyncWriter;

use super::{
    PacketDestination,
    RelayPass,
};
use crate::{
    chat::{
        receive_chat_message,
        ToChat,
    },
    client::SplinterClient,
    commands::CommandSender,
    current::proto::{
        ChatPosition,
        Packet756 as PacketLatest,
        Packet756Kind as PacketLatestKind,
        PlayClientChatMessageSpec,
        PlayServerChatMessageSpec,
    },
    events::LazyDeserializedPacket,
};

inventory::submit! {
    RelayPass(Box::new(|proxy, _connection, client, sender, lazy_packet, destination| {
        if lazy_packet.kind() == PacketLatestKind::PlayClientChatMessage {
            match lazy_packet.packet() {
                Ok(PacketLatest::PlayClientChatMessage(body)) => smol::block_on(receive_chat_message(proxy, client, sender, &body.message)),
                Ok(_) => unreachable!(),
                Err(e) => {
                    error!("Failed to deserialize chat message: {}", e);
                }
            }
            *destination = PacketDestination::None;
        }
    }))
}

impl SplinterClient {
    pub async fn send_message(
        &self,
        msg: impl ToChat,
        sender: &CommandSender,
    ) -> anyhow::Result<()> {
        self.write_packet(LazyDeserializedPacket::from_packet(
            PacketLatest::PlayServerChatMessage(PlayServerChatMessageSpec {
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
    pub async fn relay_message(&self, msg: &str) -> anyhow::Result<()> {
        self.active_server
            .load()
            .writer
            .lock()
            .await
            .write_packet_async(PacketLatest::PlayClientChatMessage(
                PlayClientChatMessageSpec {
                    message: msg.to_owned(),
                },
            ))
            .await
            .map_err(|e| e.into())
    }
}
