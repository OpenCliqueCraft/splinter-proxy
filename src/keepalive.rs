use std::{
    sync::{
        atomic::Ordering,
        Arc,
    },
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context;
use craftio_rs::{
    CraftAsyncReader,
    CraftAsyncWriter,
};
use smol::Timer;

use crate::{
    client::SplinterClient,
    current::{
        proto::PlayClientKeepAliveSpec,
        PacketLatest,
        PacketLatestKind,
        RawPacketLatest,
    },
    events::LazyDeserializedPacket,
    init::SplinterSystem,
    protocol::{
        v_cur::send_packet,
        PacketDestination,
    },
    proxy::{
        ClientKickReason,
        SplinterProxy,
    },
    server::SplinterServerConnection,
};
inventory::submit! {
    SplinterSystem {
        name: "Keep Alive",
        init: Box::new(|proxy| {
            Box::pin(keep_alive_loop(proxy))
        })
    }
}

async fn keep_alive_loop(proxy: Arc<SplinterProxy>) -> anyhow::Result<()> {
    smol::spawn(async move {
        loop {
            Timer::after(Duration::from_secs(15)).await;
            let players = proxy
                .players
                .read()
                .await
                .iter()
                .map(|(_, client)| Arc::clone(client))
                .collect::<Vec<_>>();
            let keep_alive_millis = unix_time_millis();
            for client in players.iter() {
                if keep_alive_millis - *client.last_keep_alive.lock().await > 30 * 1000 {
                    // client connection time out
                    if let Err(e) = proxy
                        .kick_client(&client.name, ClientKickReason::TimedOut)
                        .await
                    {
                        error!(
                            "Error while kicking timed out client \"{}\": {}",
                            &client.name, e
                        );
                    }
                }
            }
            let send_futs = players
                .iter()
                .map(|client| {
                    let fut = client.send_keep_alive(keep_alive_millis);
                    (client, fut)
                })
                .collect::<Vec<_>>()
                .into_iter();

            for (client, fut) in send_futs {
                if let Err(e) = fut.await {
                    error!(
                        "Failed to send keep alive packet to client \"{}\": {}",
                        &client.name, e
                    );
                }
            }
        }
    })
    .detach();
    Ok(())
}

/// Gets the current unix time in milliseconds
pub fn unix_time_millis() -> u128 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur.as_millis(),
        Err(e) => {
            warn!("System time before unix epoch?: {}", e);
            0
        }
    }
}

pub async fn watch_dummy(client: Arc<SplinterClient>, dummy_conn: Arc<SplinterServerConnection>) {
    smol::spawn(async move {
        debug!("Starting dummy watch on {} for server {}", &client.name, dummy_conn.server.id);
        loop {
            if dummy_conn.server.id == client.server_id() {
                break debug!("dummy conn server id same as active server id ({})", dummy_conn.server.id);
            }
            if !client.alive.load(Ordering::Relaxed) {
                break debug!("client for dummy conn {} no longer alive", dummy_conn.server.id);
            }
            if !dummy_conn.alive.load(Ordering::Relaxed) {
                break debug!("dummy conn {} no longer alive", dummy_conn.server.id);
            }
            let mut lock = dummy_conn.reader.lock().await;
            let raw_packet = match lock.read_raw_packet_async::<RawPacketLatest>().await {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    dummy_conn.alive.store(false, Ordering::Relaxed);
                    break debug!("Dummy connection between {} and server {} closed", &client.name, dummy_conn.server.id);
                }
                Err(e) => {
                    error!("{}-{} failed to read next raw packet: {}", &client.name, dummy_conn.server.id, e);
                    continue;
                },
            };
            let mut lazy_packet = LazyDeserializedPacket::from_raw_packet(raw_packet);
            let packet_kind = lazy_packet.kind();
            let mut pass_through = false;
            if matches!(packet_kind,
                PacketLatestKind::PlayServerKeepAlive
                | PacketLatestKind::PlayChunkData
                | PacketLatestKind::PlayUpdateLight
                | PacketLatestKind::PlayUnloadChunk) {
                match lazy_packet.packet() {
                    Ok(packet) => match packet {
                        PacketLatest::PlayServerKeepAlive(body) => {
                            let mut writer = dummy_conn.writer.lock().await;
                            debug!("{}-{} got keep alive", &client.name, dummy_conn.server.id);
                            if let Err(e) = (*writer).write_packet_async(PacketLatest::PlayClientKeepAlive(PlayClientKeepAliveSpec {
                                id: body.id
                            })).await {
                                dummy_conn.alive.store(false, Ordering::Relaxed);
                                break error!("Failed to send keep alive for dummy client between {} and server {}: {:?}", &client.name, dummy_conn.server.id, e);
                            }
                        }
                        PacketLatest::PlayChunkData(body) => {
                            let chunk = (body.x, body.z);
                            pass_through = pass_through || dummy_conn.update_chunk(&*client, true, chunk).await;
                        },
                        PacketLatest::PlayUpdateLight(body) => {
                            let chunk = (*body.chunk.x, *body.chunk.z);
                            pass_through = pass_through || dummy_conn.update_chunk(&*client, false, chunk).await;
                        },
                        PacketLatest::PlayUnloadChunk(body) => {
                            let chunk = (body.position.x, body.position.z);
                            pass_through = pass_through || dummy_conn.remove_chunk(&*client, chunk).await;
                        },
                        _ => unreachable!(),
                    }
                    Err(e) => {
                        dummy_conn.alive.store(false, Ordering::Relaxed);
                        break error!(
                            "{}-{} failed deserialize packet (type {:?}): {:?}",
                            &client.name, dummy_conn.server.id, packet_kind, e
                        )
                    }
                }
                if pass_through {
                    //debug!("passing through a {:?}", packet_kind);
                    if let Err(e) = send_packet(&client, &PacketDestination::Client, lazy_packet)
                        .await
                        .with_context(|| {
                            format!(
                                "Failed to relay packet from {}-{} to client \"{}\"",
                                &client.name, dummy_conn.server.id, &client.name
                            )
                        }) {
                        break error!("{:?}", e);
                    }
                }
            }
        }
        client.grab_dummy(dummy_conn.server.id).ok();
        debug!("Closing dummy watch on {} for server {}", &client.name, dummy_conn.server.id);
    })
    .detach()
}
