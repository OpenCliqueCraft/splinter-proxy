use std::{
    net::{
        SocketAddr,
        TcpStream,
    },
    sync::Arc,
};

use async_compat::CompatExt;
use async_dup::Arc as AsyncArc;
use craftio_rs::CraftConnection;
use mcproto_rs::protocol::PacketDirection;
use smol::Async;

use crate::protocol::{
    AsyncCraftConnection,
    AsyncCraftWriter,
};

#[derive(Clone)]
pub struct SplinterServer {
    pub id: u64,
    pub address: SocketAddr,
}

pub async fn connect(server: &Arc<SplinterServer>) -> anyhow::Result<AsyncCraftConnection> {
    let arc_stream = AsyncArc::new(Async::<TcpStream>::connect(server.address).await?);
    let (reader, writer) = (
        AsyncArc::clone(&arc_stream).compat(),
        AsyncArc::clone(&arc_stream).compat(),
    );
    let conn = CraftConnection::from_async((reader, writer), PacketDirection::ClientBound);
    Ok(conn)
}

pub struct SplinterServerConnection {
    pub writer: AsyncCraftWriter,
    pub server: Arc<SplinterServer>,
    pub alive: bool,
}
