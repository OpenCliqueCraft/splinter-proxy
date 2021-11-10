use super::{
    PacketDestination,
    RelayPass,
};
use crate::{
    protocol::current::{
        PacketLatest,
        PacketLatestKind,
    },
    proxy::{
        client::{
            ChunkLoadData,
            SplinterClient,
        },
        server::SplinterServerConnection,
    },
};

inventory::submit! {
    RelayPass(Box::new(|_proxy, connection, client, _sender, lazy_packet, destination| {
        if matches!(lazy_packet.kind(),
            PacketLatestKind::PlayChunkData
            | PacketLatestKind::PlayUpdateLight
            | PacketLatestKind::PlayUnloadChunk
        ) {
            if let Ok(packet) = lazy_packet.packet() {
                let pass_through = smol::block_on(async {
                    match packet {
                        PacketLatest::PlayChunkData(body) => {
                            let chunk = (body.x, body.z);
                            connection.update_chunk(&*client, true, chunk).await
                        },
                        PacketLatest::PlayUpdateLight(body) => {
                            let chunk = (*body.chunk.x, *body.chunk.z);
                            connection.update_chunk(&*client, false, chunk).await
                        },
                        PacketLatest::PlayUnloadChunk(body) => {
                            let chunk = (body.position.x, body.position.z);
                            connection.remove_chunk(&*client, chunk).await
                        },
                        _ => unreachable!(),
                    }
                });
                if !pass_through {
                    *destination = PacketDestination::None;
                }
            }
        }
    }))
}

impl SplinterServerConnection {
    /// Returns whether we pass the packet on
    pub async fn update_chunk(
        &self,
        client: &SplinterClient,
        is_chunkdata: bool,
        chunk: (i32, i32),
    ) -> bool {
        let newly_added_to_self = self.known_chunks.lock().await.insert(chunk);
        let client_known_chunks = &mut *client.known_chunks.lock().await;
        if let Some(load_data) = client_known_chunks.get_mut(&chunk) {
            if newly_added_to_self {
                load_data.refcount += 1;
            }
            if is_chunkdata {
                if !load_data.received_chunkdata {
                    load_data.received_chunkdata = true;
                    true
                } else {
                    false
                }
            } else {
                if !load_data.received_updatelight {
                    load_data.received_updatelight = true;
                    true
                } else {
                    false
                }
            }
        } else {
            client_known_chunks.insert(
                chunk,
                ChunkLoadData {
                    received_chunkdata: is_chunkdata,
                    received_updatelight: !is_chunkdata,
                    refcount: 1,
                },
            );
            true
        }
    }
    pub async fn remove_chunk(&self, client: &SplinterClient, chunk: (i32, i32)) -> bool {
        if self.known_chunks.lock().await.remove(&chunk) {
            let client_known_chunks = &mut *client.known_chunks.lock().await;
            if let Some(load_data) = client_known_chunks.get_mut(&chunk) {
                if load_data.refcount > 1 {
                    load_data.refcount -= 1;
                } else {
                    client_known_chunks.remove(&chunk);
                    return true;
                }
            }
        }
        return false;
    }
}
