use craftio_rs::CraftSyncWriter;
use mcproto_rs::{
    protocol::RawPacket,
    types::{
        BaseComponent,
        Chat,
        TextComponent,
    },
    v1_16_3::{
        ChatPosition,
        Packet753 as PacketLatest,
        Packet753Kind as PacketLatestKind,
        PlayClientChatMessageSpec,
        PlayServerChatMessageSpec,
        RawPacket753 as RawPacketLatest,
    },
};

use crate::{
    connection::write_packet_server,
    mapping::{
        LazyDeserializedPacket,
        PacketMap,
    },
    state::SplinterState,
};

/// Initializes chat handling
pub fn init(state: &mut SplinterState) {
    state.client_packet_map.add_action(
        PacketLatestKind::PlayClientChatMessage,
        Box::new(|client, state, lazy_packet| {
            match lazy_packet.packet() {
                Ok(packet) => {
                    if let PacketLatest::PlayClientChatMessage(data) = packet {
                        info!("{}: {}", client.name, data.message);
                        match data.message.get(..1) {
                            Some("/") => {
                                let server = client.server();
                                if let Err(e) = write_packet_server(
                                    client,
                                    &server,
                                    state,
                                    LazyDeserializedPacket::from_packet(
                                        PacketLatest::PlayClientChatMessage(
                                            PlayClientChatMessageSpec {
                                                message: data.message.clone(),
                                            },
                                        ),
                                    ),
                                ) {
                                    error!(
                                        "Failed to send command message from {}: {}",
                                        client.name, e
                                    );
                                }
                            }
                            _ => {
                                let message = format!("{}: {}", client.name, data.message);
                                for (_id, target) in state.players.read().unwrap().iter() {
                                    if let Err(e) = target.writer.lock().unwrap().write_packet(
                                        PacketLatest::PlayServerChatMessage(
                                            PlayServerChatMessageSpec {
                                                message: Chat::Text(TextComponent {
                                                    text: message.clone(),
                                                    base: BaseComponent::default(),
                                                }),
                                                position: ChatPosition::ChatBox,
                                                sender: client.uuid,
                                            },
                                        ),
                                    ) {
                                        error!(
                                            "Failed to send chat message from {} to {}: {}",
                                            client.name, target.name, e
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                Err(e) => {
                    error!("failed to deserialize chat message from player: {}", e);
                }
            };
            false
        }),
    );
}
