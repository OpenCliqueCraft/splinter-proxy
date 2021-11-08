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
        atomic::{
            AtomicBool,
            AtomicI8,
            Ordering,
        },
        Arc,
    },
};

use anyhow::Context;
use arc_swap::ArcSwap;
use async_compat::CompatExt;
use async_dup::Arc as AsyncArc;
use craftio_rs::{
    CraftAsyncReader,
    CraftAsyncWriter,
    CraftConnection,
    CraftIo,
};
use smol::{
    lock::Mutex,
    Async,
};

use crate::{
    current::{
        proto::{
            ClientStatusAction,
            PlayClientPlayerPositionAndRotationSpec,
        },
        protocol::{
            PacketDirection,
            State,
        },
        types::Vec3,
        uuid::UUID4,
        PacketLatest,
        RawPacketLatest,
    },
    keepalive::{
        self,
        watch_dummy,
    },
    mapping,
    protocol::{
        self,
        v_cur,
        AsyncCraftWriter,
    },
    proxy::SplinterProxy,
    server::SplinterServerConnection,
};

pub struct ChunkLoadData {
    pub received_chunkdata: bool,
    pub received_updatelight: bool,
    pub refcount: usize,
}

pub struct SplinterClient {
    pub name: String,
    pub writer: Mutex<AsyncCraftWriter>,
    pub alive: AtomicBool,
    pub uuid: UUID4,
    pub settings: ArcSwap<ClientSettings>,
    pub active_server: ArcSwap<SplinterServerConnection>,
    pub dummy_servers: ArcSwap<Vec<(u64, Arc<SplinterServerConnection>)>>,
    pub proxy: Arc<SplinterProxy>,
    pub last_keep_alive: Mutex<u128>,

    pub held_slot: AtomicI8,
    pub known_chunks: Mutex<HashMap<(i32, i32), ChunkLoadData>>,
    pub known_eids: Mutex<HashSet<i32>>,
    pub position: ArcSwap<Option<Vec3<f64>>>,
}
impl SplinterClient {
    pub fn new(
        proxy: Arc<SplinterProxy>,
        name: String,
        writer: AsyncCraftWriter,
        active_server: Arc<SplinterServerConnection>,
    ) -> Self {
        let uuid = mapping::uuid_from_name(&name);
        Self {
            name,
            writer: Mutex::new(writer),
            alive: AtomicBool::new(true),
            uuid,
            settings: ArcSwap::new(Arc::new(ClientSettings::default())),
            active_server: ArcSwap::new(active_server),
            dummy_servers: ArcSwap::new(Arc::new(Vec::new())),
            proxy,
            last_keep_alive: Mutex::new(keepalive::unix_time_millis()),
            held_slot: AtomicI8::new(0),
            known_chunks: Mutex::new(HashMap::new()),
            known_eids: Mutex::new(HashSet::new()),
            position: ArcSwap::new(Arc::new(None)),
        }
    }
    pub async fn set_alive(&self, value: bool) {
        self.alive.store(value, Ordering::Relaxed);
    }
    pub fn server_id(&self) -> u64 {
        self.active_server.load().server.id
    }
    pub async fn disconnect_dummy(&self, target_id: u64) -> anyhow::Result<()> {
        let dummy_servers = &**self.dummy_servers.load();
        let ind = dummy_servers
            .iter()
            .position(|v| v.0 == target_id)
            .ok_or_else(|| anyhow!("No dummy with specified target id"))?;
        let mut new_dummy_servers = dummy_servers.clone();
        let (_dummy_id, dummy) = new_dummy_servers.remove(ind);
        self.dummy_servers.store(Arc::new(new_dummy_servers));
        dummy.alive.store(false, Ordering::Relaxed);
        Ok(())
    }
    /// takes a dummy away from the client's dummy servers and returns it
    pub fn grab_dummy(&self, target_id: u64) -> anyhow::Result<Arc<SplinterServerConnection>> {
        let mut res = Err(anyhow!("somehow rcu didnt run??")); // we know the following function will run once. this error should never happen, but im not sure how to do this without setting res to an initial value
        self.dummy_servers.rcu(|servers| {
            let ind = servers.iter().position(|v| v.0 == target_id);
            if let Some(ind) = ind {
                let mut new_servers = (**servers).clone();
                res = Ok(new_servers.remove(ind).1);
                Arc::new(new_servers)
            } else {
                res = Err(anyhow!("No dummy with specified target id"));
                servers.clone()
            }
        });
        return res;
    }
    pub fn add_dummy(&self, dummy: &Arc<SplinterServerConnection>) {
        self.dummy_servers.rcu(|servers| {
            let mut new_servers = (**servers).clone();
            new_servers.push((dummy.server.id, Arc::clone(dummy)));
            Arc::new(new_servers)
        });
    }
    pub async fn swap_dummy(self: &Arc<SplinterClient>, target_id: u64) -> anyhow::Result<()> {
        // grab the dummy from the target id
        let dummy = self.grab_dummy(target_id)?;
        // remember the dummy player's eid
        let dummy_eid = dummy.eid;
        // swap the dummy connection with the active connection
        let previously_active_conn = self.active_server.swap(dummy);
        // get the ampping tables
        let mapping = &mut *self.proxy.mapping.lock().await;
        // find the corresponding proxy-side ids
        let proxy_eid = *mapping
            .eids
            .get_by_right(&(previously_active_conn.server.id, previously_active_conn.eid))
            .unwrap();
        // replace what the proxy side ids map to to the now active previously dummy eid
        mapping.eids.insert(proxy_eid, (target_id, dummy_eid));
        // put the previously active connection into the dummy connections
        self.add_dummy(&previously_active_conn);
        // watch the now dummy previously active connection
        watch_dummy(Arc::clone(self), previously_active_conn).await;
        Ok(())
    }
    pub async fn connect_dummy(self: &Arc<SplinterClient>, target_id: u64) -> anyhow::Result<()> {
        let server = Arc::clone(self.proxy.servers.read().await.get(&target_id).unwrap());
        let (server_reader, server_writer) = server
            .connect()
            .await
            .with_context(|| format!("Failed to connect dummy to server {}", target_id))?
            .into_split();
        let mut server_conn = SplinterServerConnection {
            writer: Mutex::new(server_writer),
            reader: Mutex::new(server_reader),
            server: (*server).clone(),
            alive: AtomicBool::new(true),
            eid: -1,
            uuid: UUID4::from(0u128),
            known_chunks: Mutex::new(HashSet::new()),
        };

        // let mut player_position = None;

        v_cur::send_handshake(&mut server_conn, &self.proxy).await?;
        server_conn.writer.get_mut().set_state(State::Login);
        server_conn.reader.get_mut().set_state(State::Login);
        v_cur::send_login_start(&mut server_conn, &self.name).await?;
        loop {
            let packet = server_conn
                .reader
                .get_mut()
                .read_packet_async::<RawPacketLatest>()
                .await?;
            match packet {
                Some(PacketLatest::LoginEncryptionRequest(_)) => bail!(
                    "Failed to connect to server {} because it requested encryption",
                    target_id
                ),
                Some(PacketLatest::LoginSetCompression(body)) => {
                    let threshold = if *body.threshold > 0 {
                        Some(*body.threshold)
                    } else {
                        None
                    };
                    server_conn
                        .writer
                        .get_mut()
                        .set_compression_threshold(threshold);
                    server_conn
                        .reader
                        .get_mut()
                        .set_compression_threshold(threshold);
                }
                Some(PacketLatest::LoginSuccess(body)) => {
                    server_conn.uuid = body.uuid;
                    server_conn.writer.get_mut().set_state(State::Play);
                    server_conn.reader.get_mut().set_state(State::Play);
                }
                Some(PacketLatest::PlayJoinGame(body)) => {
                    server_conn.eid = body.entity_id;
                    // note: we do not map here. any mapping would get in the way of the active
                    // connections main eid mapping
                    // send brand here if wanted, but its not really necessary
                    v_cur::send_client_settings(
                        &mut server_conn,
                        (&**self.settings.load()).clone(),
                    )
                    .await?;
                }
                Some(PacketLatest::PlayServerPluginMessage(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayServerDifficulty(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayServerPlayerAbilities(_body)) => {
                    // ignore
                    // TODO: may need to do something with this ex. transitioning a player between servers when theyre flying
                }
                Some(PacketLatest::PlayServerHeldItemChange(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayDeclareRecipes(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayTags(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayEntityStatus(_body)) => {
                    // ignore
                    // *probably* doesnt matter
                }
                Some(PacketLatest::PlayDeclareCommands(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayUnlockRecipes(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayServerPlayerPositionAndLook(body)) => {
                    v_cur::send_teleport_confirm(&mut server_conn, body.teleport_id).await?;
                    server_conn
                        .writer
                        .get_mut()
                        .write_packet_async(PacketLatest::PlayClientPlayerPositionAndRotation(
                            PlayClientPlayerPositionAndRotationSpec {
                                feet_location: body.location,
                                on_ground: false,
                            },
                        ))
                        .await
                        .map_err(|e| anyhow!(e))?;
                    v_cur::send_client_status(&mut server_conn, ClientStatusAction::PerformRespawn)
                        .await?;
                    v_cur::send_held_item_change(
                        &mut server_conn,
                        self.held_slot.load(Ordering::Relaxed),
                    )
                    .await?;
                    break;
                }
                Some(PacketLatest::PlayPlayerInfo(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayUpdateViewPosition(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayUpdateLight(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlayChunkData(_body)) => {
                    // ignore
                }
                Some(PacketLatest::PlaySpawnPosition(_body)) => {
                    // ignore
                }
                Some(packet) => warn!("Unexpected packet during login {:?}", packet),
                None => bail!("Connection attempt to server {} closed", target_id),
            }
        }
        let arc_conn = Arc::new(server_conn);
        self.add_dummy(&arc_conn);
        watch_dummy(Arc::clone(self), arc_conn).await;
        Ok(())
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
