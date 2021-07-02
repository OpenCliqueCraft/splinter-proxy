use std::{
    collections::{
        HashMap,
        HashSet,
    },
    iter::FromIterator,
    net::{
        SocketAddr,
        TcpStream,
    },
    sync::{
        Arc,
        RwLock,
    },
};

use arc_swap::ArcSwap;
use async_compat::CompatExt;
use async_dup::Arc as AsyncArc;
use craftio_rs::{
    CraftAsyncWriter,
    CraftConnection,
};
use mcproto_rs::{
    protocol::{
        HasPacketKind,
        PacketDirection,
        RawPacket,
    },
    uuid::UUID4,
};
use smol::{
    lock::Mutex,
    Async,
};

// use self::events::ClientEvents;
use crate::{
    chat::ToChat,
    // client::events::ProxyToServerDispatcher,
    commands::CommandSender,
    events::LazyDeserializedPacket,
    mapping,
    protocol::{
        self,
        version::{
            V753,
            V755,
        },
        AsyncCraftWriter,
        ConnectionVersion,
        ProtocolVersion,
    },
    proxy::{
        ClientKickReason,
        SplinterProxy,
    },
    server::SplinterServerConnection,
};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum ClientVersion {
    V753,
    V755,
}

pub struct SplinterClient {
    pub name: String,
    pub writer: Mutex<AsyncCraftWriter>,
    pub version: ClientVersion,
    pub alive: ArcSwap<bool>,
    pub uuid: UUID4,
    pub settings: ArcSwap<ClientSettings>,
    pub servers: Mutex<HashMap<u64, Arc<Mutex<SplinterServerConnection>>>>,
    pub active_server_id: ArcSwap<u64>,
    pub proxy: Arc<SplinterProxy>,
}
impl SplinterClient {
    pub fn new(
        proxy: Arc<SplinterProxy>,
        name: String,
        writer: AsyncCraftWriter,
        version: ClientVersion,
    ) -> Self {
        Self {
            name: name.clone(),
            version,
            writer: Mutex::new(writer),
            alive: ArcSwap::new(Arc::new(true)),
            uuid: mapping::uuid::uuid_from_bytes(format!("OfflinePlayer:{}", &name).as_bytes()),
            settings: ArcSwap::new(Arc::new(ClientSettings::default())),
            servers: Mutex::new(HashMap::new()), /* TODO: put active server in its own specially accessible property */
            active_server_id: ArcSwap::new(Arc::new(0)),
            proxy,
        }
    }
    pub fn set_name(&mut self, name: String) {
        self.name = name;
        self.uuid =
            mapping::uuid::uuid_from_bytes(format!("OfflinePlayer:{}", &self.name).as_bytes());
    }
    pub async fn send_message(
        &self,
        chat: impl ToChat,
        sender: &CommandSender,
    ) -> anyhow::Result<()> {
        match self.version {
            ClientVersion::V753 => self.send_message_v753(chat, sender).await,
            ClientVersion::V755 => todo!(),
        }
    }
    pub async fn send_kick(&self, reason: ClientKickReason) -> anyhow::Result<()> {
        match self.version {
            ClientVersion::V753 => self.send_kick_v753(reason).await,
            ClientVersion::V755 => todo!(),
        }
    }
    pub async fn set_alive(&self, value: bool) {
        self.alive.store(Arc::new(value));
    }
    pub async fn relay_message(&self, msg: &str, server_id: u64) -> anyhow::Result<()> {
        match self.version {
            ClientVersion::V753 => self.relay_message_v753(msg, server_id).await,
            ClientVersion::V755 => todo!(),
        }
    }
    pub fn server_id(&self) -> u64 {
        **self.active_server_id.load()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum ChatMode {
    Enabled,
    CommandsOnly,
    Hidden,
}
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum SkinPart {
    Cape,
    Jacket,
    LeftSleeve,
    RightSleeve,
    LeftPant,
    RightPant,
    Hat,
}
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum MainHand {
    Left,
    Right,
}
#[derive(Clone)]
pub struct ClientSettings {
    pub locale: String,
    pub view_distance: i8,
    pub chat_mode: ChatMode,
    pub chat_colors: bool,
    pub skin_parts: HashSet<SkinPart>,
    pub main_hand: MainHand,
}
impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            locale: "en_US".into(),
            view_distance: 8,
            chat_mode: ChatMode::Enabled,
            chat_colors: true,
            skin_parts: HashSet::from_iter([
                SkinPart::Jacket,
                SkinPart::LeftSleeve,
                SkinPart::RightSleeve,
                SkinPart::LeftPant,
                SkinPart::RightPant,
                SkinPart::Hat,
            ]),
            main_hand: MainHand::Right,
        }
    }
}

pub fn handle(
    stream: Async<TcpStream>,
    addr: SocketAddr,
    proxy: Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    let arc_stream = AsyncArc::new(stream);
    let (reader, writer) = (
        AsyncArc::clone(&arc_stream).compat(),
        AsyncArc::clone(&arc_stream).compat(),
    );
    let conn = CraftConnection::from_async((reader, writer), PacketDirection::ServerBound);
    smol::spawn(async move {
        // wait for initial handshake
        if let Err(e) = match proxy.protocol {
            ProtocolVersion::V753 | ProtocolVersion::V754 => {
                protocol::handle_handshake(conn, addr, proxy).await
            }
            ProtocolVersion::V755 => todo!(),
        } {
            error!("Failed to handle handshake: {}", e);
        }
    })
    .detach();
    Ok(())
}
