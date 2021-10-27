use std::{
    net::{
        SocketAddr,
        TcpStream,
    },
    sync::Arc,
};

use arc_swap::ArcSwap;
use async_compat::CompatExt;
use async_dup::Arc as AsyncArc;
use craftio_rs::CraftConnection;
use mcproto_rs::protocol::PacketDirection;
use smol::{
    lock::Mutex,
    Async,
};

use crate::{
    client::SplinterClient,
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
    pub server: Arc<SplinterServer>,
    pub alive: ArcSwap<bool>,
}

impl SplinterServerConnection {
    pub fn _is_dummy(&self, client: &SplinterClient) -> bool {
        client.server_id() != self.server.id
    }
}
