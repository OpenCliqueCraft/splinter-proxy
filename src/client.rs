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
    sync::Arc,
};

use arc_swap::ArcSwap;
use async_compat::CompatExt;
use async_dup::Arc as AsyncArc;
use craftio_rs::CraftConnection;
use mcproto_rs::{
    protocol::PacketDirection,
    uuid::UUID4,
};
use smol::{
    lock::Mutex,
    Async,
};

use crate::{
    keepalive,
    mapping,
    protocol::{
        self,
        AsyncCraftWriter,
    },
    proxy::SplinterProxy,
    server::SplinterServerConnection,
};

pub struct SplinterClient {
    pub name: String,
    pub writer: Mutex<AsyncCraftWriter>,
    pub alive: ArcSwap<bool>,
    pub uuid: UUID4,
    pub settings: ArcSwap<ClientSettings>,
    pub active_server: ArcSwap<SplinterServerConnection>,
    pub dummy_servers: Mutex<HashMap<u64, Arc<SplinterServerConnection>>>,
    pub proxy: Arc<SplinterProxy>,
    pub last_keep_alive: Mutex<u128>,
}
impl SplinterClient {
    pub fn new(
        proxy: Arc<SplinterProxy>,
        name: String,
        writer: AsyncCraftWriter,
        active_server: Arc<SplinterServerConnection>,
    ) -> Self {
        let uuid = mapping::uuid::uuid_from_name(&name);
        Self {
            name,
            writer: Mutex::new(writer),
            alive: ArcSwap::new(Arc::new(true)),
            uuid,
            settings: ArcSwap::new(Arc::new(ClientSettings::default())),
            active_server: ArcSwap::new(active_server),
            dummy_servers: Mutex::new(HashMap::new()),
            proxy,
            last_keep_alive: Mutex::new(keepalive::unix_time_millis()),
        }
    }
    pub async fn set_alive(&self, value: bool) {
        self.alive.store(Arc::new(value));
    }
    pub fn server_id(&self) -> u64 {
        self.active_server.load().server.id
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
    pub text_filtering_enabled: bool,
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
            text_filtering_enabled: false,
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
        if let Err(e) = protocol::handle_handshake(conn, addr, proxy).await {
            error!("Failed to handle handshake: {:?}", e,);
        }
    })
    .detach();
    Ok(())
}
