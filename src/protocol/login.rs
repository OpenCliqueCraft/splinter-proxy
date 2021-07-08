use std::{
    net::SocketAddr,
    sync::Arc,
};

use anyhow::Context;
use arc_swap::ArcSwap;
use craftio_rs::CraftIo;
use futures_lite::future;
use mcproto_rs::{
    protocol::{
        PacketDirection,
        State,
    },
    uuid::UUID4,
};
use smol::lock::Mutex;

use super::{
    AsyncCraftConnection,
    AsyncCraftReader,
    AsyncCraftWriter,
    Tags,
};
use crate::{
    client::{
        ClientSettings,
        ClientVersion,
        SplinterClient,
    },
    mapping::{
        uuid::uuid_from_name,
        SplinterMapping,
    },
    protocol::{
        v753,
        v755,
    },
    proxy::SplinterProxy,
    server::SplinterServerConnection,
};

pub struct ClientBuilder<'a> {
    pub proxy: &'a Arc<SplinterProxy>,
    pub version: &'a ClientVersion,
    pub name: Option<String>,
    pub uuid: Option<UUID4>,
    pub client_addr: SocketAddr,
    pub client_writer: AsyncCraftWriter,
    pub server_conn: Option<SplinterServerConnection>,
    pub settings: Option<ClientSettings>,
}

impl<'a> ClientBuilder<'a> {
    pub fn new(
        proxy: &'a Arc<SplinterProxy>,
        client_addr: SocketAddr,
        version: &'a ClientVersion,
        client_writer: AsyncCraftWriter,
    ) -> Self {
        Self {
            proxy,
            version,
            name: None,
            uuid: None,
            client_addr,
            server_conn: None,
            client_writer,
            settings: None,
        }
    }
    pub async fn login_start(
        &mut self,
        name: impl AsRef<str>,
        server_conn_reader_opt: &mut Option<AsyncCraftReader>,
    ) -> anyhow::Result<()> {
        debug!("login start");
        self.name = Some(name.as_ref().to_owned());
        self.uuid = Some(uuid_from_name(name));
        info!(
            "\"{}\" logging in from {}",
            self.name.as_ref().unwrap(),
            self.client_addr
        );
        let active_server_id = 0u64; // todo: zoning
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
        let (mut server_reader, server_writer) = server_craft_conn.into_split();
        let mut server_conn = SplinterServerConnection {
            writer: Mutex::new(server_writer),
            server: Arc::clone(&server),
            alive: ArcSwap::new(Arc::new(true)),
            map: Mutex::new(SplinterMapping::new()),
        };
        info!(
            "Connection for client \"{}\" initiated with {}",
            self.name.as_ref().unwrap(),
            server.address
        );
        send_handshake(&mut server_conn, self.proxy, self.version)
            .await
            .with_context(|| {
                format!(
                    "Failed to write handshake to server {}, {}",
                    server_conn.server.id, server_conn.server.address,
                )
            })?;
        server_conn.writer.get_mut().set_state(State::Login);
        server_reader.set_state(State::Login);
        send_login_start(&mut server_conn, self.version, self.name.as_ref().unwrap())
            .await
            .with_context(|| {
                format!(
                    "Failed to write login start packet to server {}, {}",
                    active_server_id, server.address
                )
            })?;
        self.server_conn = Some(server_conn);
        *server_conn_reader_opt = Some(server_reader);
        debug!("login start end");
        Ok(())
    }
    pub fn login_set_compression(&mut self, threshold: i32, server_conn_reader: &mut impl CraftIo) {
        debug!("login set compression");
        let threshold = if threshold > 0 { Some(threshold) } else { None };
        self.server_conn
            .as_mut()
            .unwrap()
            .writer
            .get_mut()
            .set_compression_threshold(threshold);
        server_conn_reader.set_compression_threshold(threshold);
    }
    pub async fn login_success(
        &mut self,
        client_conn_reader: &mut impl CraftIo,
        server_conn_reader: &mut impl CraftIo,
    ) -> anyhow::Result<()> {
        debug!("login success");
        if let Some(threshold) = self.proxy.config.compression_threshold {
            send_set_compression(&mut self.client_writer, self.version, threshold)
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
        send_login_success(
            &mut self.client_writer,
            self.version,
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
        self.server_conn
            .as_mut()
            .unwrap()
            .writer
            .get_mut()
            .set_state(State::Play);
        server_conn_reader.set_state(State::Play);
        debug!("login success end");
        Ok(())
    }
    pub async fn play_join_game(&mut self) -> anyhow::Result<()> {
        debug!("play join game");
        const MAX_BRAND_SIZE: usize = 128;
        let brand = if self.proxy.config.brand.len() >= MAX_BRAND_SIZE {
            &self.proxy.config.brand[0..MAX_BRAND_SIZE]
        } else {
            self.proxy.config.brand.as_str()
        };
        send_brand(&mut self.client_writer, self.version, brand)
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
        send_client_settings(
            self.server_conn.as_mut().unwrap(),
            self.version,
            settings_clone,
        )
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
            send_tags(&mut self.client_writer, self.version, &tags)
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
            send_tags(&mut self.client_writer, self.version, &tags)
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
    pub fn build(self) -> SplinterClient {
        let cl = SplinterClient::new(
            Arc::clone(self.proxy),
            self.name.unwrap(),
            self.client_writer,
            *self.version,
            Arc::new(self.server_conn.unwrap()),
        );
        cl.settings.store(Arc::new(self.settings.unwrap()));
        cl
    }
}

pub async fn send_handshake(
    server_conn: &mut SplinterServerConnection,
    proxy: &Arc<SplinterProxy>,
    version: &ClientVersion,
) -> anyhow::Result<()> {
    match version {
        ClientVersion::V753 => v753::send_handshake_v753(server_conn, proxy).await,
        ClientVersion::V755 => v755::send_handshake_v755(server_conn, proxy).await,
    }
}
pub async fn send_login_start(
    server_conn: &mut SplinterServerConnection,
    version: &ClientVersion,
    name: impl ToString,
) -> anyhow::Result<()> {
    match version {
        ClientVersion::V753 => v753::send_login_start_v753(server_conn, name).await,
        ClientVersion::V755 => v755::send_login_start_v755(server_conn, name).await,
    }
}
pub async fn send_set_compression(
    writer: &mut AsyncCraftWriter,
    version: &ClientVersion,
    threshold: i32,
) -> anyhow::Result<()> {
    match version {
        ClientVersion::V753 => v753::send_set_compression_v753(writer, threshold).await,
        ClientVersion::V755 => v755::send_set_compression_v755(writer, threshold).await,
    }
}
pub async fn send_login_success(
    writer: &mut AsyncCraftWriter,
    version: &ClientVersion,
    name: String,
    uuid: UUID4,
) -> anyhow::Result<()> {
    match version {
        ClientVersion::V753 => v753::send_login_success_v753(writer, name, uuid).await,
        ClientVersion::V755 => v755::send_login_success_v755(writer, name, uuid).await,
    }
}
pub async fn send_brand(
    writer: &mut AsyncCraftWriter,
    version: &ClientVersion,
    brand: impl AsRef<str>,
) -> anyhow::Result<()> {
    match version {
        ClientVersion::V753 => v753::send_brand_v753(writer, brand).await,
        ClientVersion::V755 => v755::send_brand_v755(writer, brand).await,
    }
}
pub async fn send_client_settings(
    server_conn: &mut SplinterServerConnection,
    version: &ClientVersion,
    settings: ClientSettings,
) -> anyhow::Result<()> {
    match version {
        ClientVersion::V753 => v753::send_client_settings_v753(server_conn, settings).await,
        ClientVersion::V755 => v755::send_client_settings_v755(server_conn, settings).await,
    }
}
pub async fn send_tags(
    writer: &mut AsyncCraftWriter,
    version: &ClientVersion,
    tags: &Tags,
) -> anyhow::Result<()> {
    match version {
        ClientVersion::V753 => v753::send_tags_v753(writer, tags).await,
        ClientVersion::V755 => v755::send_tags_v755(writer, tags).await,
    }
}

pub async fn handle_client_login(
    mut conn: AsyncCraftConnection,
    version: ClientVersion,
    addr: SocketAddr,
    proxy: Arc<SplinterProxy>,
) -> anyhow::Result<()> {
    conn.set_state(State::Login);
    let (mut client_conn_reader, client_conn_writer) = conn.into_split();
    let mut client_builder = ClientBuilder::new(&proxy, addr, &version, client_conn_writer);
    let mut server_conn_reader: Option<AsyncCraftReader> = None;
    let mut next_sender = PacketDirection::ServerBound;
    loop {
        if let Some(val) = match version {
            ClientVersion::V753 => {
                v753::handle_client_login_packet(
                    &mut next_sender,
                    &mut client_builder,
                    &mut server_conn_reader,
                    &mut client_conn_reader,
                )
                .await
            }
            ClientVersion::V755 => {
                v755::handle_client_login_packet(
                    &mut next_sender,
                    &mut client_builder,
                    &mut server_conn_reader,
                    &mut client_conn_reader,
                )
                .await
            }
        }
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
    let client = client_builder.build();
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
            server_conn_reader.unwrap(),
        ),
    )
    .await;
    res_a?;
    res_b?;
    Ok(())
}
