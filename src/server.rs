use std::{
    collections::HashSet,
    net::{
        SocketAddr,
        TcpStream,
    },
    sync::atomic::AtomicBool,
};

use async_compat::CompatExt;
use async_dup::Arc as AsyncArc;
use craftio_rs::CraftConnection;
use mcproto_rs::protocol::PacketDirection;
use smol::{
    lock::Mutex,
    Async,
};

use crate::{
    client::{
        ChunkLoadData,
        SplinterClient,
    },
    current::uuid::UUID4,
    protocol::{
        AsyncCraftConnection,
        AsyncCraftReader,
        AsyncCraftWriter,
    },
};

#[derive(Clone)]
pub struct SplinterServer {
    pub id: u64,
    pub address: SocketAddr,
}
impl SplinterServer {
    pub async fn connect(&self) -> anyhow::Result<AsyncCraftConnection> {
        let arc_stream = AsyncArc::new(Async::<TcpStream>::connect(self.address).await?);
        let (reader, writer) = (
            AsyncArc::clone(&arc_stream).compat(),
            AsyncArc::clone(&arc_stream).compat(),
        );
        let conn = CraftConnection::from_async((reader, writer), PacketDirection::ClientBound);
        Ok(conn)
    }
}

pub struct SplinterServerConnection {
    pub writer: Mutex<AsyncCraftWriter>,
    pub reader: Mutex<AsyncCraftReader>,
    pub server: SplinterServer,
    pub alive: AtomicBool,

    pub eid: i32,
    pub uuid: UUID4,
    pub known_chunks: Mutex<HashSet<(i32, i32)>>,
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
