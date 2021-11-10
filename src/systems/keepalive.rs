use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, SystemTime},
};

use anyhow::Context;
use craftio_rs::{CraftAsyncReader, CraftAsyncWriter};
use mcproto_rs::protocol::PacketDirection;
use smol::Timer;

use crate::{
    protocol::{
        current::{
            proto::{PlayClientKeepAliveSpec, PlayTeleportConfirmSpec},
            PacketLatest, PacketLatestKind, RawPacketLatest,
        },
        events::LazyDeserializedPacket,
        v_cur::{has_eids, map_eid, send_packet, send_position_set},
        PacketDestination,
    },
    proxy::{
        client::SplinterClient, mapping::SplinterMappingResult, server::SplinterServerConnection,
        ClientKickReason, SplinterProxy,
    },
    systems::SplinterSystem,
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
                | PacketLatestKind::PlayUnloadChunk
                | PacketLatestKind::PlayTeleportConfirm
                | PacketLatestKind::PlayServerPlayerPositionAndLook) {
                match lazy_packet.packet() {
                    Ok(packet) => match packet {
                        PacketLatest::PlayServerKeepAlive(body) => {
                            let mut writer = dummy_conn.writer.lock().await;
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
                        PacketLatest::PlayServerPlayerPositionAndLook(body) => {
                            debug!("Desynchronization! {}-{} asked to teleport!", &client.name, dummy_conn.server.id);
                            let writer = &mut *dummy_conn.writer.lock().await;
                            if let Err(e) = writer.write_packet_async(PacketLatest::PlayTeleportConfirm(PlayTeleportConfirmSpec {
                                teleport_id: body.teleport_id,
                            })).await {
                                dummy_conn.alive.store(false, Ordering::Relaxed);
                                break error!("Failed to respond to dummy teleport request for {}-{}: {:?}", &client.name, dummy_conn.server.id, e);
                            }
                            // if the position the server wants us to go to is farther than where
                            // we actually should be, then send a position set to the plugin

                            // as a note here, this only handles when the provided teleportation
                            // request has an absolute position. TODO: relative position
                            if body.flags.0 == 0 {
                                let tpos = body.location.position;
                                let ppos = &**client.position.load();
                                const MAX_DIST: f64 = 15.;
                                if (tpos.x - ppos.x).abs() > MAX_DIST || (tpos.y - ppos.y).abs() > MAX_DIST || (tpos.z - ppos.z).abs() > MAX_DIST {
                                    if let Err(e) = send_position_set(writer, ppos.x, ppos.y, ppos.z).await {
                                        dummy_conn.alive.store(false, Ordering::Relaxed);
                                        break error!("Failed to send position set to dummy {}-{}: {:?}", &client.name, dummy_conn.server.id, e);
                                    }
                                }
                            }
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
            }
            if has_eids(lazy_packet.kind()) {
                if let Ok(packet) = lazy_packet.packet() {
                    let map = &mut *client.proxy.mapping.lock().await;
                    pass_through = pass_through || SplinterMappingResult::Client == map_eid(&*client, map, packet, &PacketDirection::ClientBound, &dummy_conn.server);
                }
            }
            if pass_through {
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
        client.grab_dummy(dummy_conn.server.id).ok();
        debug!("Closing dummy watch on {} for server {}", &client.name, dummy_conn.server.id);
    })
    .detach()
}
