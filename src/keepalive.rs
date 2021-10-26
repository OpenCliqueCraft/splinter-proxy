use std::{
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

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
        RawPacketLatest,
    },
    init::SplinterSystem,
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
        loop {
            if dummy_conn.server.id == client.server_id() || !**client.alive.load() {
                break;
            }
            let mut lock = dummy_conn.reader.lock().await;
            match lock.read_packet_async::<RawPacketLatest>().await {
                Ok(Some(packet)) => match packet {
                    PacketLatest::PlayServerKeepAlive(body) => {
                        let mut writer = dummy_conn.writer.lock().await;
                        if let Err(e) = (*writer).write_packet_async(PacketLatest::PlayClientKeepAlive(PlayClientKeepAliveSpec {
                            id: body.id
                        })).await {
                            return error!("Failed to send keep alive for dummy client between {} and server {}: {}", &client.name, dummy_conn.server.id, e);
                        }
                    }
                    _ => {
                        //ignore all other packets
                    }
                }
                Ok(None) => {
                    return debug!("Dummy connection between {} and server {} closed", &client.name, dummy_conn.server.id);
                }
                Err(e) => {
                    return error!(
                        "Error reading incoming packet for dummy connection between {} and server {}: {}",
                        &client.name, dummy_conn.server.id, e
                    )
                }
            }
        }
    })
    .detach()
}
