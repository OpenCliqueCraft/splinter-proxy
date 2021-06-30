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

use self::events::ClientEvents;
use crate::{
    chat::ToChat,
    client::events::ProxyToServerDispatcher,
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
    proxy::SplinterProxy,
    server::SplinterServerConnection,
};

pub struct SplinterClient<T>
where
    for<'a> T: ConnectionVersion<'a>,
{
    pub name: String,
    pub writer: Mutex<AsyncCraftWriter>,
    pub alive: Mutex<bool>,
    pub uuid: UUID4,
    pub settings: Mutex<ClientSettings>,
    pub servers: Mutex<HashMap<u64, Arc<Mutex<SplinterServerConnection>>>>,
    pub active_server_id: u64,
    pub event: Mutex<ClientEvents<T>>,
    pub proxy: Arc<SplinterProxy>,
}
impl<T> SplinterClient<T>
where
    for<'a> T: ConnectionVersion<'a>,
{
    pub fn new(proxy: Arc<SplinterProxy>, name: String, writer: AsyncCraftWriter) -> Self {
        Self {
            name: name.clone(),
            writer: Mutex::new(writer),
            alive: Mutex::new(true),
            uuid: mapping::uuid::uuid_from_bytes(format!("OfflinePlayer:{}", &name).as_bytes()),
            settings: Mutex::new(ClientSettings::default()),
            servers: Mutex::new(HashMap::new()),
            active_server_id: 0,
            event: Mutex::new(ClientEvents {
                proxy_to_server: ProxyToServerDispatcher {
                    listeners: vec![],
                    action: Box::new(|proxy, event| {}), // TODO
                },
            }),
            proxy,
        }
    }
    pub fn set_name(&mut self, name: String) {
        self.name = name;
        self.uuid =
            mapping::uuid::uuid_from_bytes(format!("OfflinePlayer:{}", &self.name).as_bytes());
    }
}

pub enum SplinterClientVersion {
    V753(SplinterClient<V753>),
    V755(SplinterClient<V755>),
}
impl SplinterClientVersion {
    pub fn name<'a>(&'a self) -> &'a str {
        match self {
            SplinterClientVersion::V753(client) => client.name.as_str(),
            SplinterClientVersion::V755(client) => client.name.as_str(),
        }
    }
    pub fn uuid(&self) -> UUID4 {
        match self {
            SplinterClientVersion::V753(client) => client.uuid,
            SplinterClientVersion::V755(client) => client.uuid,
        }
    }
    pub async fn send_message(
        &self,
        chat: impl ToChat,
        sender: &CommandSender,
    ) -> anyhow::Result<()> {
        match self {
            SplinterClientVersion::V753(client) => client.send_message(chat, sender).await,
            SplinterClientVersion::V755(client) => todo!(), // client.send_message(chat).await,
        }
    }
}

pub mod events {
    use std::sync::Arc;

    use mcproto_rs::protocol::{
        HasPacketKind,
        RawPacket,
    };

    use super::SplinterClient;
    use crate::{
        events::{
            LazyDeserializedPacket,
            SplinterEventFn,
        },
        protocol::ConnectionVersion,
        proxy::SplinterProxy,
    };

    pub struct ClientEvents<T>
    where
        for<'a> T: ConnectionVersion<'a>,
    {
        pub proxy_to_server: ProxyToServerDispatcher<T>,
        // pub server_to_proxy: SplinterEventDispatcher<ClientEventServerToProxy>,
    }
    pub struct ProxyToServerDispatcher<T>
    where
        for<'a> T: ConnectionVersion<'a>,
    {
        pub listeners:
            Vec<Box<dyn Send + Sync + for<'b> FnMut(&SplinterProxy, &mut ProxyToServer<'b, T>)>>,
        pub action: Box<dyn Send + Sync + for<'b> FnMut(&SplinterProxy, &mut ProxyToServer<'b, T>)>,
    }
    //
    pub struct ProxyToServer<'b, T>
    where
        for<'a> T: ConnectionVersion<'a>,
    {
        pub cancelled: bool,
        pub target_server_id: u64,
        pub client: &'b SplinterClient<T>,
        pub packet: &'b mut LazyDeserializedPacket<'b, T>,
    }
    impl<'b, T> ProxyToServer<'b, T>
    where
        for<'a> T: ConnectionVersion<'a>,
    {
        fn is_cancelled(&self) -> bool {
            self.cancelled
        }
        fn set_cancelled(&mut self, cancelled: bool) {
            self.cancelled = cancelled;
        }
    }
    pub struct ServerToProxy<'b, T>
    where
        for<'a> T: ConnectionVersion<'a>,
        for<'a> <<T as ConnectionVersion<'a>>::Protocol as RawPacket<'a>>::Packet: HasPacketKind,
    {
        pub cancelled: bool,
        pub client: &'b SplinterClient<T>,
        pub packet: &'b mut LazyDeserializedPacket<'b, T>,
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
