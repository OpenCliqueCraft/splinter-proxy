use std::{
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use craftio_rs::CraftAsyncWriter;
use mcproto_rs::v1_16_3::{
    Packet753,
    Packet753Kind,
    PlayClientKeepAliveSpec,
};
use smol::Timer;

use crate::{
    init::SplinterSystem,
    protocol::{
        v753,
        v755,
        PacketDestination,
        PacketSender,
    },
    proxy::{
        ClientKickReason,
        SplinterProxy,
    },
};
inventory::submit! {
    SplinterSystem {
        name: "Keep Alive",
        init: Box::new(|proxy| {
            Box::pin(keep_alive_loop(proxy))
        })
    }
}

inventory::submit! {
    v753::RelayPass(Box::new(|_proxy, sender, lazy_packet, _map, destination| {
        match sender {
            PacketSender::Proxy(client) => {
                if lazy_packet.kind() == Packet753Kind::PlayClientKeepAlive { // TODO: may want to do something with the keep alive IDs
                    *smol::block_on(client.last_keep_alive.lock()) = unix_time_millis();
                    *destination = PacketDestination::None;
                }
            }
            PacketSender::Server(server, client) => {
                if lazy_packet.kind() == Packet753Kind::PlayServerKeepAlive {
                    if let Ok(Packet753::PlayServerKeepAlive(body)) = lazy_packet.packet() {
                    // respond to server
                    let servers = smol::block_on(client.servers.lock());
                    let mut server_conn = smol::block_on(servers.get(&server.id).unwrap().lock());
                    if let Err(e) = smol::block_on(server_conn.writer.write_packet_async(Packet753::PlayClientKeepAlive(PlayClientKeepAliveSpec {
                        id: body.id,
                    }))) {
                        error!("Failed to send keep alive from \"{}\" to server id {}: {}", &client.name, server.id, e);
                    }
                }
                }
            }
        }
    }))
}
async fn keep_alive_loop(proxy: Arc<SplinterProxy>) -> anyhow::Result<()> {
    smol::spawn(async move {
        loop {
            Timer::after(Duration::from_secs(15)).await;
            let players = proxy
                .players
                .read()
                .unwrap()
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
