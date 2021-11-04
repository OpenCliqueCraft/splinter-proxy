use std::{
    collections::HashMap,
    fmt::Debug,
    net::{
        SocketAddr,
        TcpStream,
    },
    sync::{
        atomic::Ordering,
        Arc,
    },
};

use async_compat::Compat;
use async_dup::Arc as AsyncArc;
use craftio_rs::{
    CraftAsyncReader,
    CraftConnection,
    CraftReader,
    CraftWriter,
};
use smol::Async;

use crate::{
    client::SplinterClient,
    current::{
        proto::{
            HandshakeNextState,
            Packet756 as PacketLatest,
            RawPacket756 as RawPacketLatest,
        },
        protocol::PacketDirection,
    },
    proxy::SplinterProxy,
    server::SplinterServerConnection,
};

mod login;
pub mod v_cur;
pub use login::*;

pub type AsyncCraftConnection =
    CraftConnection<Compat<AsyncArc<Async<TcpStream>>>, Compat<AsyncArc<Async<TcpStream>>>>;
pub type AsyncCraftWriter = CraftWriter<Compat<AsyncArc<Async<TcpStream>>>>;
pub type AsyncCraftReader = CraftReader<Compat<AsyncArc<Async<TcpStream>>>>;

/// Wrapper for a hashmap of tags corresponding to a list of namespaced ids.
#[derive(Clone, Debug)]
pub struct TagList(HashMap<String, Vec<String>>);

/// Contains tags for the tag lists of blocks, items, entities, and fluids.
#[derive(Clone, Debug)]
pub struct Tags {
    pub tags: HashMap<String, TagList>,
}

/// Loads a JSON file into a Vec of i32 and String pairs
///
/// Expects the JSON file to be in the format of a list of objects, and each object has a `name`
/// string and an `id` number.
fn load_json_id_name_pairs(data: impl AsRef<str>) -> Vec<(i32, String)> {
    let parsed = match json::parse(data.as_ref()) {
        Ok(parsed) => parsed,
        Err(e) => {
            error!("Failed to parse json: {}", e);
            panic!("File parse error");
        }
    };
    let mut list = vec![];
    for block_data in parsed.members() {
        list.push((
            block_data["id"]
                .as_i32()
                .expect("Failed to convert JSON id to i32"),
            block_data["name"]
                .as_str()
                .expect("Failed to convert JSON name to str")
                .into(),
        ));
    }
    list
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketDestination {
    None,
    Server(u64),
    AllServers,
    Client,
}

pub async fn handle_handshake(
    mut conn: AsyncCraftConnection,
    addr: SocketAddr,
    proxy: Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    let packet = conn.read_packet_async::<RawPacketLatest>().await?;
    match packet {
        Some(PacketLatest::Handshake(body)) => match body.next_state {
            HandshakeNextState::Status => v_cur::handle_client_status(conn, addr, proxy).await?,
            HandshakeNextState::Login => {
                handle_client_login(conn, addr, proxy).await?;
            }
        },
        Some(other_packet) => bail!(
            "Expected a handshake packet; instead got: {:?}",
            other_packet
        ),
        None => {}
    }
    Ok(())
}

impl SplinterClient {
    pub async fn handle_server_relay(
        self: &Arc<Self>,
        proxy: Arc<SplinterProxy>,
        server_conn: Arc<SplinterServerConnection>,
        client: Arc<SplinterClient>,
    ) -> anyhow::Result<()> {
        let server = server_conn.server.clone();
        let sender = PacketDirection::ClientBound;
        loop {
            // server->proxy->client
            if !self.alive.load(Ordering::Relaxed) || !server_conn.alive.load(Ordering::Relaxed) {
                break;
            }
            let active_server = client.active_server.load();
            let server_reader: &mut AsyncCraftReader = &mut *active_server.reader.lock().await;
            match v_cur::handle_server_packet(&proxy, self, server_reader, &server, &sender).await {
                Ok(Some(())) => {}
                Ok(None) => break, // connection closed
                Err(e) => {
                    error!("Failed to handle packet from server: {:?}", e);
                }
            }
        }
        server_conn.alive.store(false, Ordering::Relaxed);
        debug!(
            "Server connection between {} and server id {} closed",
            self.name, server.id
        );
        Ok(())
    }
    pub async fn handle_client_relay(
        self: &Arc<Self>,
        proxy: Arc<SplinterProxy>,
        mut client_reader: AsyncCraftReader,
    ) -> anyhow::Result<()> {
        let sender = PacketDirection::ServerBound;
        loop {
            // client->proxy->server
            if !self.alive.load(Ordering::Relaxed) {
                break;
            }
            match v_cur::handle_client_packet(&proxy, self, &mut client_reader, &sender).await {
                Ok(Some(())) => {}
                Ok(None) => break,
                Err(e) => {
                    error!(
                        "Failed to handle packet from client \"{}\": {}",
                        &self.name, e
                    );
                }
            }
        }
        proxy.players.write().await.remove(&self.name);
        self.alive.store(false, Ordering::Relaxed);
        info!("Client \"{}\" connection closed", self.name);
        Ok(())
    }
}
