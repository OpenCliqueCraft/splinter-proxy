use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{atomic::AtomicBool, Arc},
};

use anyhow::Context;
use craftio_rs::CraftIo;
use futures_lite::future;
use smol::lock::Mutex;

use super::{v_cur::send_position_set, AsyncCraftConnection, AsyncCraftWriter, Tags};
use crate::{
    protocol::{
        current::{
            protocol::{PacketDirection, State},
            types::Vec3,
            uuid::UUID4,
        },
        v_cur,
    },
    proxy::{
        client::{ClientSettings, SplinterClient},
        mapping::uuid_from_name,
        server::SplinterServerConnection,
        SplinterProxy,
    },
    systems::{playersave::DEFAULT_SPAWN_POSITION, zoning::world_to_chunk_position},
};

pub struct ClientBuilder<'a> {
    pub proxy: &'a Arc<SplinterProxy>,
    pub name: Option<String>,
    pub uuid: Option<UUID4>,
    pub client_addr: SocketAddr,
    pub client_writer: AsyncCraftWriter,
    pub server_conn: Option<SplinterServerConnection>,
    pub settings: Option<ClientSettings>,
    pub position: Option<Vec3<f64>>,
}

impl<'a> ClientBuilder<'a> {
    pub fn new(
        proxy: &'a Arc<SplinterProxy>,
        client_addr: SocketAddr,
        client_writer: AsyncCraftWriter,
    ) -> Self {
        Self {
            proxy,
            name: None,
            uuid: None,
            client_addr,
            server_conn: None,
            client_writer,
            settings: None,
            position: None,
        }
    }
    pub async fn login_start(&mut self, name: impl AsRef<str>) -> anyhow::Result<()> {
        self.name = Some(name.as_ref().to_owned());
        self.uuid = Some(uuid_from_name(name));
        info!(
            "\"{}\" logging in from {}",
            self.name.as_ref().unwrap(),
            self.client_addr
        );
        let player_data_lock = self.proxy.player_data.lock().await;
        let plinfo = player_data_lock.players.get(self.uuid.as_ref().unwrap());
        let spawn_pos = if let Some(plinfo) = plinfo {
            self.position = Some((plinfo.x, plinfo.y, plinfo.z).into());
            (plinfo.x, plinfo.z)
        } else {
            self.position = Some(DEFAULT_SPAWN_POSITION.into());
            (DEFAULT_SPAWN_POSITION.0, DEFAULT_SPAWN_POSITION.2)
        };
        debug!("spawn position is {:?}", self.position.as_ref().unwrap());
        let active_server_id = *self
            .proxy
            .zoner
            .zones_in_point(world_to_chunk_position(spawn_pos))
            .get(0)
            .unwrap_or(&0);
        debug!("player should join server {}", active_server_id);
        let server = Arc::clone(
            self.proxy
                .servers
                .read()
                .await
                .get(&active_server_id)
                .unwrap(),
        );
        let server_craft_conn = server
            .connect()
            .await
            .with_context(|| "Failed to connect client to server")?;
        let (server_reader, server_writer) = server_craft_conn.into_split();
        let mut server_conn = SplinterServerConnection {
            writer: Mutex::new(server_writer),
            reader: Mutex::new(server_reader),
            server: (*server).clone(),
            alive: AtomicBool::new(true),
            eid: -1,
            uuid: UUID4::from(0u128),
            known_chunks: Mutex::new(HashSet::new()),
        };
        info!(
            "Connection for client \"{}\" initiated with {}",
            self.name.as_ref().unwrap(),
            server.address
        );
        v_cur::send_handshake(&mut server_conn, self.proxy)
            .await
            .with_context(|| {
                format!(
                    "Failed to write handshake to server {}, {}",
                    server_conn.server.id, server_conn.server.address,
                )
            })?;
        server_conn.writer.get_mut().set_state(State::Login);
        server_conn.reader.get_mut().set_state(State::Login);
        v_cur::send_login_start(&mut server_conn, self.name.as_ref().unwrap())
            .await
            .with_context(|| {
                format!(
                    "Failed to write login start packet to server {}, {}",
                    active_server_id, server.address
                )
            })?;
        self.server_conn = Some(server_conn);
        Ok(())
    }
    pub fn login_set_compression(&mut self, threshold: i32) {
        let threshold = if threshold > 0 { Some(threshold) } else { None };
        let conn = self.server_conn.as_mut().unwrap();
        conn.writer.get_mut().set_compression_threshold(threshold);
        conn.reader.get_mut().set_compression_threshold(threshold);
    }
    pub async fn login_success(
        &mut self,
        client_conn_reader: &mut impl CraftIo,
    ) -> anyhow::Result<()> {
        if let Some(threshold) = self.proxy.config.compression_threshold {
            v_cur::send_set_compression(&mut self.client_writer, threshold)
                .await
                .with_context(|| {
                    format!(
                        "Failed to send compression packet to {}",
                        self.name.as_ref().unwrap()
                    )
                })?;
            self.client_writer
                .set_compression_threshold(Some(threshold));
            client_conn_reader.set_compression_threshold(Some(threshold));
        }
        v_cur::send_login_success(
            &mut self.client_writer,
            self.name.as_ref().unwrap().to_owned(),
            self.uuid.unwrap(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to relay login packet to client {}",
                self.name.as_ref().unwrap(),
            )
        })?;
        client_conn_reader.set_state(State::Play);
        self.client_writer.set_state(State::Play);
        let conn = self.server_conn.as_mut().unwrap();
        conn.writer.get_mut().set_state(State::Play);
        conn.reader.get_mut().set_state(State::Play);
        Ok(())
    }
    pub async fn play_join_game(&mut self) -> anyhow::Result<()> {
        const MAX_BRAND_SIZE: usize = 128;
        let brand = if self.proxy.config.brand.len() >= MAX_BRAND_SIZE {
            &self.proxy.config.brand[0..MAX_BRAND_SIZE]
        } else {
            self.proxy.config.brand.as_str()
        };
        v_cur::send_brand(&mut self.client_writer, brand)
            .await
            .with_context(|| {
                format!(
                    "Failed to send brand to client {}",
                    self.name.as_ref().unwrap()
                )
            })?;
        Ok(())
    }
    pub async fn play_client_settings(&mut self, settings: ClientSettings) -> anyhow::Result<()> {
        let settings_clone = settings.clone();
        self.settings = Some(settings);
        v_cur::send_client_settings(self.server_conn.as_mut().unwrap(), settings_clone)
            .await
            .with_context(|| {
                format!(
                    "Failed to relay client settings from {} to server {}",
                    self.name.as_ref().unwrap(),
                    self.server_conn.as_ref().unwrap().server.id,
                )
            })?;
        let tags_opt = self.proxy.tags.lock().await.as_ref().cloned();
        if let Some(tags) = tags_opt {
            v_cur::send_tags(&mut self.client_writer, &tags)
                .await
                .with_context(|| {
                    format!(
                        "Failed to send tags packet to client {}",
                        self.name.as_ref().unwrap(),
                    )
                })?;
        }
        Ok(())
    }
    pub async fn play_tags(&mut self, tags: Tags) -> anyhow::Result<()> {
        if self.proxy.tags.lock().await.is_none() {
            v_cur::send_tags(&mut self.client_writer, &tags)
                .await
                .with_context(|| {
                    format!(
                        "Failed to send tags packet to client {}",
                        self.name.as_ref().unwrap(),
                    )
                })?;
            *self.proxy.tags.lock().await = Some(tags);
        }
        Ok(())
    }
    pub async fn build(self) -> SplinterClient {
        let cl = SplinterClient::new(
            Arc::clone(self.proxy),
            self.name.unwrap(),
            self.client_writer,
            Arc::new(self.server_conn.unwrap()),
            self.position.unwrap(),
        );
        cl.settings.store(Arc::new(self.settings.unwrap()));
        cl
    }
}

pub async fn handle_client_login(
    mut conn: AsyncCraftConnection,
    addr: SocketAddr,
    proxy: Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    conn.set_state(State::Login);
    let (mut client_conn_reader, client_conn_writer) = conn.into_split();
    let mut client_builder = ClientBuilder::new(&proxy, addr, client_conn_writer);
    let mut next_sender = PacketDirection::ServerBound;
    loop {
        if let Some(val) = v_cur::handle_client_login_packet(
            &mut next_sender,
            &mut client_builder,
            &mut client_conn_reader,
        )
        .await
        .with_context(|| "Handling login packet")?
        {
            if val {
                break;
            }
        } else {
            bail!(
                "Client \"{}\", {} connection closed during login",
                client_builder.name.unwrap_or_else(String::new),
                addr,
            );
        }
    }
    let client = client_builder.build().await;
    let cl_pos = &**client.position.load();
    send_position_set(
        &mut *client.active_server.load().writer.lock().await,
        cl_pos.x,
        cl_pos.y,
        cl_pos.z,
    )
    .await
    .with_context(|| "Sending position set")?;
    let client_arc = Arc::new(client);
    proxy
        .players
        .write()
        .await
        .insert(client_arc.name.clone(), Arc::clone(&client_arc));

    // move on to relay loop
    let (res_a, res_b) = future::zip(
        client_arc.handle_client_relay(Arc::clone(&proxy), client_conn_reader),
        client_arc.handle_server_relay(
            proxy,
            Arc::clone(&*client_arc.active_server.load()),
            Arc::clone(&client_arc),
        ),
    )
    .await;
    res_a?;
    res_b?;
    Ok(())
}
