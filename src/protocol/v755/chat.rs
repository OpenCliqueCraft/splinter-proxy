use craftio_rs::CraftAsyncWriter;
use mcproto_rs::v1_17_0::{
    ChatPosition,
    Packet755,
    Packet755Kind,
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
    protocol::{
        version::V755,
        PacketDestination,
    },
};

inventory::submit! {
    RelayPass(Box::new(|proxy, sender, lazy_packet, _map, destination| {
        if lazy_packet.kind() == Packet755Kind::PlayClientChatMessage {
            match lazy_packet.packet() {
                Ok(Packet755::PlayClientChatMessage(body)) => smol::block_on(receive_chat_message(proxy, sender, &body.message)),
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
    pub async fn send_message_v755(
        &self,
        msg: impl ToChat,
        sender: &CommandSender,
    ) -> anyhow::Result<()> {
        self.write_packet_v755(LazyDeserializedPacket::<V755>::from_packet(
            Packet755::PlayServerChatMessage(PlayServerChatMessageSpec {
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
    pub async fn relay_message_v755(&self, msg: &str, server_id: u64) -> anyhow::Result<()> {
        let servers = self.servers.lock().await;
        let mut server_conn = servers
            .get(&server_id)
            .ok_or_else(|| {
                anyhow!(
                    "Failed to get server connection for server id \"{}\"",
                    server_id
                )
            })?
            .lock()
            .await;
        server_conn
            .writer
            .write_packet_async(Packet755::PlayClientChatMessage(
                PlayClientChatMessageSpec {
                    message: msg.to_owned(),
                },
            ))
            .await
            .map_err(|e| anyhow!("{}", e))
    }
}
